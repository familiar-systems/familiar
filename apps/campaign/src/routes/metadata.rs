//! `GET /campaign/<id>` -- campaign metadata handler.

use crate::actors::database_writer::{GetMetadata, MetadataError};
use crate::actors::registry::GetCampaign;
use crate::middleware::auth::AuthenticatedUser;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::onboarding::metadata::CampaignMetadataResponse;
use fs_id::Nanoid;

#[utoipa::path(
    get,
    path = "/campaign/{id}",
    tag = "campaign",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    responses(
        (status = OK, description = "Campaign metadata", body = CampaignMetadataResponse),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Not the campaign owner"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = 503, description = "Server restarting"),
        (status = 500, description = "Internal error"),
    ),
)]
pub async fn get_campaign(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
) -> impl IntoResponse {
    let supervisor = match state
        .registry
        .ask(GetCampaign(CampaignId::from(Nanoid::from(
            campaign_id.clone(),
        ))))
        .await
    {
        Ok(Some(sup)) => sup,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    match supervisor.ask(GetMetadata).await {
        Ok(model) => {
            if model.owner_user_id != user.id.to_string() {
                return StatusCode::FORBIDDEN.into_response();
            }
            let resp = CampaignMetadataResponse {
                campaign_id,
                name: model.name,
                tagline: model.tagline,
                game_system: model.game_system,
                content_locale: model.content_locale,
                wizard_completed_at: model.wizard_completed_at.map(|dt| dt.to_rfc3339()),
                created_at: model.created_at.to_rfc3339(),
                updated_at: model.updated_at.to_rfc3339(),
            };
            (StatusCode::OK, Json(resp)).into_response()
        }
        Err(kameo::error::SendError::HandlerError(MetadataError::NoMetadataRow)) => {
            StatusCode::NOT_FOUND.into_response()
        }
        Err(kameo::error::SendError::HandlerError(_)) => {
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
