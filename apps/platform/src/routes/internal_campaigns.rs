//! Internal callback routes (`/internal/platform/campaigns/*`).
//!
//! These are pod-to-pod calls from the campaign tier; not exposed via the
//! public Caddy/Traefik path matchers. The internal-bearer middleware is
//! mounted on the parent router (`/internal/platform/`) so every handler
//! here is bearer-checked.

use crate::{entities::campaigns, error::AppError, state::AppState};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use chrono::Utc;
use familiar_systems_app_shared::campaigns::internal::{InitFailedRequest, MetadataMirrorRequest};
use sea_orm::{ActiveValue::Set, EntityTrait};

/// `POST /internal/platform/campaigns/<id>/metadata`: mirror campaign
/// metadata from the campaign tier onto the platform's routing row. Called
/// after a successful wizard seal so the hub list can render campaign names
/// without per-shard fan-out.
pub async fn report_metadata(
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(body): Json<MetadataMirrorRequest>,
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
    am.name = Set(Some(body.name));
    am.tagline = Set(body.tagline);
    am.game_system = Set(Some(body.game_system));
    am.content_locale = Set(Some(body.content_locale));
    am.wizard_completed_at = Set(Some(body.wizard_completed_at));
    am.updated_at = Set(now);
    campaigns::Entity::update(am).exec(&state.db).await?;

    Ok(StatusCode::OK)
}

/// `POST /internal/platform/campaigns/<id>/init-failed`: record that the
/// campaign tier tried to initialize this campaign and failed. The platform
/// persists `reason` onto `campaigns.last_init_error` so the SPA can render
/// an "initialization failed" badge in the hub list.
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
        // Race: campaign tier reported failure for a campaign whose routing
        // row hasn't been written yet. The platform's create flow writes
        // the row only after a successful shard init, so this is genuinely
        // anomalous; log and accept.
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
