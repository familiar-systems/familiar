//! Public campaign routes (`/api/campaigns`).
//!
//! Per the platform CLAUDE.md, paths here are post-strip — declare
//! `/campaigns`, browsers reach it at `/api/campaigns`.

use crate::{
    clients::campaign_internal::CampaignInternalError,
    entities::{campaigns, create_attempts},
    error::AppError,
    middleware::auth::AuthenticatedUser,
    state::AppState,
};
use axum::{Json, extract::State};
use chrono::Utc;
use familiar_systems_app_shared::{
    campaigns::api::{Campaign, CreateCampaignRequest, CreateCampaignResponse},
    id::{CampaignId, UserId},
};
use fs_id::Nanoid;
use sea_orm::{
    ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, sea_query::OnConflict,
};

/// `POST /api/campaigns` — mint a CampaignId, ask the campaign tier to
/// initialize per-campaign state, write the routing row, return the id.
///
/// Idempotent on `idempotency_token`: a retry with the same token returns
/// the same `campaign_id`. The order (write `create_attempts` first, then
/// call shard, then write `campaigns`) is what makes retries safe — see
/// the long form in the design doc.
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
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Json(body): Json<CreateCampaignRequest>,
) -> Result<Json<CreateCampaignResponse>, AppError> {
    // Step 1: idempotency probe. If we have a record for this token, return
    // the previously minted campaign_id without doing any work.
    if let Some(existing) = create_attempts::Entity::find_by_id(body.idempotency_token.clone())
        .one(&state.db)
        .await?
    {
        return Ok(Json(CreateCampaignResponse {
            campaign_id: CampaignId::new(Nanoid(existing.campaign_id)),
        }));
    }

    // Step 2: mint a fresh id.
    let campaign_id = CampaignId::generate();
    let user_id = UserId(user.id);
    let shard_url = state.config.campaign_shard_url.clone();
    let now = Utc::now();

    // Step 3: claim the idempotency token. INSERT OR IGNORE so a concurrent
    // racer who beat us writes their (token, campaign_id) and we re-read
    // theirs. After this point, every retry of the same logical call
    // resolves to the same campaign_id.
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

    // Step 4: shard init. Idempotent on the campaign tier's side, so a retry
    // (whether ours or someone else's) re-runs cleanly.
    state
        .campaign_internal
        .init(&resolved_campaign_id, &user_id)
        .await
        .map_err(|e| match e {
            CampaignInternalError::Transport(err) => {
                AppError::Internal(format!("campaign init transport: {err}"))
            }
            CampaignInternalError::Status { status } => {
                AppError::Internal(format!("campaign init status: {status}"))
            }
        })?;

    // Step 5: routing row. INSERT OR IGNORE on PK so retries no-op.
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
    .exec(&state.db)
    .await?;

    Ok(Json(CreateCampaignResponse {
        campaign_id: resolved_campaign_id,
    }))
}

/// `GET /api/campaigns` — list the authenticated user's campaigns,
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
    user: AuthenticatedUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<Campaign>>, AppError> {
    let rows = campaigns::Entity::find()
        .filter(campaigns::Column::OwnerUserId.eq(user.id))
        .order_by_desc(campaigns::Column::CreatedAt)
        .all(&state.db)
        .await?;
    let out: Vec<Campaign> = rows
        .into_iter()
        .map(|m| Campaign {
            id: CampaignId::new(Nanoid(m.id)),
            name: m.name,
            tagline: m.tagline,
            game_system: m.game_system,
            content_locale: m.content_locale,
            last_init_error: m.last_init_error,
            wizard_completed_at: m.wizard_completed_at,
            created_at: m.created_at,
            updated_at: m.updated_at,
        })
        .collect();
    Ok(Json(out))
}
