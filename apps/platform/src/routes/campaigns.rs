//! Public campaign routes (`/api/campaigns`).
//!
//! Per the platform CLAUDE.md, paths here are post-strip: declare
//! `/campaigns`, browsers reach it at `/api/campaigns`.

use crate::{
    clients::campaign_internal::CampaignInternalError,
    entities::{campaign_members, campaigns, create_attempts},
    error::AppError,
    middleware::auth::PlatformUser,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use familiar_systems_app_shared::{
    campaigns::api::{Campaign, CreateCampaignRequest, CreateCampaignResponse},
    id::{CampaignId, UserId},
};
use fs_id::Nanoid;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, TransactionTrait,
    sea_query::OnConflict,
};

/// `POST /api/campaigns`: mint a CampaignId, ask the campaign tier to
/// initialize per-campaign state, write the routing row, return the id.
///
/// Idempotent on `idempotency_token`: a retry with the same token returns
/// the same `campaign_id`. The order (write `create_attempts` first, then
/// call shard, then write `campaigns`) is what makes retries safe; see
/// the long form in the design doc.
///
/// Crucially, the token is *not* a "done" marker. Every step after the token
/// claim is idempotent (shard create + lease are idempotent on `campaign_id`;
/// the platform inserts are `INSERT … ON CONFLICT DO NOTHING`), so a retry
/// after a partial failure re-drives the missing steps and converges to a
/// complete campaign. A benign duplicate of an already-complete campaign is
/// detected by the GM membership row (the last write) and short-circuits.
#[utoipa::path(
    post,
    path = "/campaigns",
    tag = "campaigns",
    request_body = CreateCampaignRequest,
    responses(
        (status = OK, description = "Campaign created", body = CreateCampaignResponse),
        (status = UNAUTHORIZED, description = "Authentication required"),
        (status = INTERNAL_SERVER_ERROR, description = "Shard or DB failure"),
    ),
    security(("bearerAuth" = [])),
)]
pub async fn create_campaign(
    user: PlatformUser,
    State(state): State<AppState>,
    Json(body): Json<CreateCampaignRequest>,
) -> Result<Json<CreateCampaignResponse>, AppError> {
    let user_id = UserId(user.id);
    let now = Utc::now();

    // Step 1: claim-or-recover the idempotency token. INSERT OR IGNORE so a
    // concurrent racer who beat us keeps their (token, campaign_id); we then
    // re-read to learn the authoritative id. Token-first is deliberate: it
    // dedupes concurrent requests and pins the campaign_id before the shard
    // call, so a retry never mints a second shard campaign. (`create_attempts`
    // has no FK to `campaigns` precisely because it is written first.)
    let campaign_id = CampaignId::generate();
    create_attempts::Entity::insert(create_attempts::ActiveModel {
        idempotency_token: Set(body.idempotency_token.clone()),
        campaign_id: Set(campaign_id.0.0.clone()),
        created_at: Set(now),
    })
    .on_conflict(
        OnConflict::column(create_attempts::Column::IdempotencyToken)
            .do_nothing()
            .to_owned(),
    )
    .do_nothing()
    .exec(&state.db)
    .await?;

    let resolved = create_attempts::Entity::find_by_id(body.idempotency_token.clone())
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::Internal("idempotency record vanished after upsert".into()))?;
    let resolved_campaign_id = CampaignId::new(Nanoid(resolved.campaign_id));

    // Step 2: completeness fast-path. The GM membership row is the *last*
    // write of a successful create, so its presence proves every prior step
    // (shard create, lease, routing row) finished. A benign duplicate of a
    // fully-provisioned campaign returns here without re-touching the shard.
    // If it is absent the token was claimed by an attempt that then died
    // mid-flight; fall through and re-drive the (idempotent) provisioning to
    // heal it, rather than returning a stale success for a dead campaign.
    if campaign_members::Entity::find_by_id((resolved_campaign_id.0.0.clone(), user.id))
        .one(&state.db)
        .await?
        .is_some()
    {
        return Ok(Json(CreateCampaignResponse {
            campaign_id: resolved_campaign_id,
        }));
    }

    // Step 3a: create the campaign on the shard. Idempotent on campaign_id.
    state
        .campaign_internal
        .create_campaign(&resolved_campaign_id, &user_id)
        .await
        .map_err(|e| match e {
            CampaignInternalError::Transport(err) => {
                AppError::Internal(format!("campaign create transport: {err}"))
            }
            CampaignInternalError::Status { status } => {
                AppError::Internal(format!("campaign create status: {status}"))
            }
        })?;

    // Step 3b: acquire the lease (ensure it's checked out). For a just-created
    // campaign this is already true; the call is here for uniformity with the
    // cold-checkout flow.
    state
        .campaign_internal
        .acquire_lease(&resolved_campaign_id)
        .await
        .map_err(|e| match e {
            CampaignInternalError::Transport(err) => {
                AppError::Internal(format!("lease acquire transport: {err}"))
            }
            CampaignInternalError::Status { status } => {
                AppError::Internal(format!("lease acquire status: {status}"))
            }
        })?;

    // Step 4: routing row + GM seed, committed atomically. One transaction so
    // a crash between them can never leave a routing row (campaign visible in
    // the owner's hub) without its membership row (every WS upgrade / Thing
    // create would 403 the owner). With the resume path above, the only
    // partial states are "nothing platform-side yet" (next retry heals) or
    // "complete". `campaigns` is inserted before `campaign_members` so the FK
    // ordering is correct once the platform DB enforces foreign keys. Both are
    // INSERT OR IGNORE so a re-drive after a partial failure no-ops cleanly.
    let shard_url = state.config.campaign_shard_url.clone();
    let txn = state.db.begin().await?;

    campaigns::Entity::insert(campaigns::ActiveModel {
        id: Set(resolved_campaign_id.0.0.clone()),
        owner_user_id: Set(user.id),
        shard_url: Set(shard_url),
        name: Set(None),
        tagline: Set(None),
        game_system: Set(None),
        content_locale: Set(None),
        last_init_error: Set(None),
        wizard_completed_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    })
    .on_conflict(
        OnConflict::column(campaigns::Column::Id)
            .do_nothing()
            .to_owned(),
    )
    .do_nothing()
    .exec(&txn)
    .await?;

    campaign_members::Entity::insert(campaign_members::ActiveModel {
        campaign_id: Set(resolved_campaign_id.0.0.clone()),
        user_id: Set(user.id),
        role: Set(campaign_members::CampaignRole::Gm),
        created_at: Set(now),
    })
    .on_conflict(
        OnConflict::columns([
            campaign_members::Column::CampaignId,
            campaign_members::Column::UserId,
        ])
        .do_nothing()
        .to_owned(),
    )
    .do_nothing()
    .exec(&txn)
    .await?;

    txn.commit().await?;

    if let Ok(mut cache) = state.loaded_cache.write() {
        cache.insert(resolved_campaign_id.clone());
    }

    Ok(Json(CreateCampaignResponse {
        campaign_id: resolved_campaign_id,
    }))
}

/// `GET /api/campaigns`: list the authenticated user's campaigns,
/// most-recent first. Reads only the platform-side routing table; no
/// fan-out to shards.
#[utoipa::path(
    get,
    path = "/campaigns",
    tag = "campaigns",
    responses(
        (status = OK, description = "List of campaigns", body = Vec<Campaign>),
        (status = UNAUTHORIZED, description = "Authentication required"),
    ),
    security(("bearerAuth" = [])),
)]
pub async fn list_campaigns(
    user: PlatformUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Campaign>>, AppError> {
    let rows = campaigns::Entity::find()
        .filter(campaigns::Column::OwnerUserId.eq(user.id))
        .order_by_desc(campaigns::Column::CreatedAt)
        .all(&state.db)
        .await?;
    let cache = state.loaded_cache.read().ok();
    let out: Vec<Campaign> = rows
        .into_iter()
        .map(|m| {
            let cid = CampaignId::new(Nanoid(m.id));
            let loaded = cache.as_ref().is_some_and(|c| c.contains(&cid));
            Campaign {
                id: cid,
                name: m.name,
                tagline: m.tagline,
                game_system: m.game_system,
                content_locale: m.content_locale,
                last_init_error: m.last_init_error,
                loaded,
                wizard_completed_at: m.wizard_completed_at,
                created_at: m.created_at,
                updated_at: m.updated_at,
            }
        })
        .collect();
    Ok(Json(out))
}

/// `GET /api/campaigns/{id}`: fetch a single campaign and ensure it is
/// loaded on its shard. The SPA calls this before talking to the campaign
/// server; the implicit lease acquisition is invisible to the caller.
///
/// Returns 404 for both "not found" and "not owned by this user" to
/// prevent campaign-ID enumeration.
#[utoipa::path(
    get,
    path = "/campaigns/{id}",
    tag = "campaigns",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    responses(
        (status = OK, description = "Campaign details", body = Campaign),
        (status = UNAUTHORIZED, description = "Authentication required"),
        (status = NOT_FOUND, description = "Campaign not found or not owned"),
        (status = INTERNAL_SERVER_ERROR, description = "Shard failure"),
    ),
    security(("bearerAuth" = [])),
)]
pub async fn get_campaign(
    user: PlatformUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
) -> Result<(StatusCode, Json<Campaign>), AppError> {
    let row = campaigns::Entity::find_by_id(campaign_id.clone())
        .one(&state.db)
        .await?
        .ok_or(AppError::NotFound)?;

    if row.owner_user_id != user.id {
        return Err(AppError::NotFound);
    }

    let cid = CampaignId::new(Nanoid(campaign_id));
    state
        .campaign_internal
        .acquire_lease(&cid)
        .await
        .map_err(|e| match e {
            CampaignInternalError::Transport(err) => {
                AppError::Internal(format!("lease acquire transport: {err}"))
            }
            CampaignInternalError::Status { status } => {
                AppError::Internal(format!("lease acquire status: {status}"))
            }
        })?;

    if let Ok(mut cache) = state.loaded_cache.write() {
        cache.insert(cid.clone());
    }

    Ok((
        StatusCode::OK,
        Json(Campaign {
            id: cid,
            name: row.name,
            tagline: row.tagline,
            game_system: row.game_system,
            content_locale: row.content_locale,
            last_init_error: row.last_init_error,
            loaded: true,
            wizard_completed_at: row.wizard_completed_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }),
    ))
}
