//! Internal callback routes (`/internal/platform/campaign/*`).
//!
//! These are pod-to-pod calls from the campaign tier; not exposed via the
//! public Caddy/Traefik path matchers. The internal-bearer middleware is
//! mounted on the parent router (`/internal/platform/`) so every handler
//! here is bearer-checked.

use crate::{
    entities::{campaign_members, campaigns},
    error::AppError,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use familiar_systems_app_shared::campaigns::internal::{
    HeartbeatRequest, InitFailedRequest, MembershipResponse, PatchCampaignMirror,
};
use familiar_systems_app_shared::id::CampaignId;
use fs_id::Nanoid;
use sea_orm::{ActiveValue::Set, EntityTrait};
use uuid::Uuid;

/// `PATCH /internal/platform/campaign/{id}`: mirror changed campaign
/// metadata from the campaign tier onto the platform's routing row. Fires
/// after every successful metadata update so the hub list stays current
/// without per-shard fan-out.
pub async fn patch_campaign(
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(body): Json<PatchCampaignMirror>,
) -> Result<StatusCode, AppError> {
    let now = Utc::now();

    let existing = campaigns::Entity::find_by_id(campaign_id.clone())
        .one(&state.db)
        .await?;
    let Some(model) = existing else {
        tracing::warn!(
            campaign_id = %campaign_id,
            "metadata mirror for unknown campaign id"
        );
        return Ok(StatusCode::OK);
    };

    let mut am: campaigns::ActiveModel = model.into();
    if let Some(name) = body.name {
        am.name = Set(Some(name));
    }
    if let Some(tagline) = body.tagline {
        am.tagline = Set(Some(tagline));
    }
    if let Some(game_system) = body.game_system {
        am.game_system = Set(Some(game_system));
    }
    if let Some(content_locale) = body.content_locale {
        am.content_locale = Set(Some(content_locale));
    }
    if let Some(wca) = body.wizard_completed_at {
        am.wizard_completed_at = Set(Some(wca));
    }
    am.updated_at = Set(now);
    campaigns::Entity::update(am).exec(&state.db).await?;

    Ok(StatusCode::OK)
}

/// `DELETE /internal/platform/campaign/{id}/lease`: the campaign shard
/// notifies the platform that it released a campaign (idle eviction).
/// Removes the campaign from the in-memory loaded cache.
pub async fn release_lease(
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
) -> StatusCode {
    let cid = CampaignId::new(Nanoid(campaign_id.clone()));
    if let Ok(mut cache) = state.loaded_cache.write() {
        cache.remove(&cid);
    }
    tracing::info!(
        campaign_id = %campaign_id,
        "shard reported campaign lease released"
    );
    StatusCode::OK
}

/// `POST /internal/platform/heartbeat`: the campaign shard sends the list
/// of currently loaded campaign IDs. Replaces the loaded cache wholesale,
/// reconciling any drift from missed release notifications or restarts.
pub async fn heartbeat(
    State(state): State<AppState>,
    Json(body): Json<HeartbeatRequest>,
) -> StatusCode {
    let count = body.campaigns.len();
    if let Ok(mut cache) = state.loaded_cache.write() {
        cache.clear();
        cache.extend(body.campaigns);
    }
    tracing::debug!(loaded_count = count, "heartbeat received");
    StatusCode::OK
}

/// `POST /internal/platform/campaign/{id}/init-failed`: record that the
/// campaign tier tried to complete the wizard and failed. The platform
/// persists `reason` onto `campaigns.last_init_error` so the SPA can
/// render an "initialization failed" badge in the hub list.
///
/// Returns 200 even if the campaign id doesn't exist yet (the campaign tier
/// shouldn't have to coordinate with the platform's view of when the row
/// landed): a missing row is logged at warn and a 200 still goes back. This
/// keeps the campaign tier's failure path stable.
///
/// **Not** exported via the public OpenAPI: `#[utoipa::path]` is omitted on
/// purpose so the spec stays scoped to the SPA contract.
pub async fn report_init_failed(
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(body): Json<InitFailedRequest>,
) -> Result<StatusCode, AppError> {
    let now = Utc::now();

    let existing = campaigns::Entity::find_by_id(campaign_id.clone())
        .one(&state.db)
        .await?;
    let Some(model) = existing else {
        tracing::warn!(
            campaign_id = %campaign_id,
            reason = %body.reason,
            "init-failed callback for unknown campaign id"
        );
        return Ok(StatusCode::OK);
    };

    let mut am: campaigns::ActiveModel = model.into();
    am.last_init_error = Set(Some(body.reason));
    am.updated_at = Set(now);
    campaigns::Entity::update(am).exec(&state.db).await?;

    Ok(StatusCode::OK)
}

/// `GET /internal/platform/campaign/{id}/membership/{user_id}`: check
/// whether a user is a member of a campaign and return their functional
/// role.
///
/// Called by the campaign tier at WebSocket upgrade time (once per
/// connection, not per message). Returns 200 + [`MembershipResponse`]
/// if the user is a member, 404 if not.
pub async fn check_membership(
    State(state): State<AppState>,
    Path((campaign_id, user_id)): Path<(String, String)>,
) -> Result<Json<MembershipResponse>, StatusCode> {
    let user_uuid = Uuid::parse_str(&user_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let row = campaign_members::Entity::find_by_id((campaign_id.clone(), user_uuid))
        .one(&state.db)
        .await
        .map_err(|e| {
            tracing::error!(
                campaign_id = %campaign_id,
                user_id = %user_id,
                error = %e,
                "membership check query failed"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        })?;

    let Some(member) = row else {
        return Err(StatusCode::NOT_FOUND);
    };

    Ok(Json(MembershipResponse {
        role: member.role.into(),
    }))
}
