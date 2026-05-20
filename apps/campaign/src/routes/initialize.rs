//! `POST /campaign/<id>/initialize` -- campaign initialization handler.
//!
//! Writes campaign metadata (name, tagline, game_system, content_locale)
//! and sets `wizard_completed_at`. Template instantiation is deferred to
//! a follow-up slice; the `template_slugs` field is accepted but ignored.

use crate::actors::database_writer::{GetMetadata, InitializeCampaignError};
use crate::actors::registry::GetCampaign;
use crate::actors::supervisor::InitializeCampaign;
use crate::middleware::auth::AuthenticatedUser;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use familiar_systems_app_shared::campaigns::internal::MetadataMirrorRequest;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::onboarding::initialize::{
    InitializeErrorResponse, InitializeRequest,
};
use fs_id::Nanoid;

#[utoipa::path(
    post,
    path = "/campaign/{id}/initialize",
    tag = "campaign",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    request_body = InitializeRequest,
    responses(
        (status = OK, description = "Campaign initialized"),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Not the campaign owner"),
        (status = 409, description = "Already initialized"),
        (status = 500, description = "Initialization failed", body = InitializeErrorResponse),
    ),
)]
pub async fn initialize(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<InitializeRequest>,
) -> impl IntoResponse {
    tracing::info!(
        campaign_id = %campaign_id,
        game_system = %req.game_system,
        content_locale = %req.content_locale,
        template_count = req.template_slugs.len(),
        audio = ?req.audio,
        evals_enabled = req.evals_enabled,
        "initializing campaign"
    );

    let supervisor = match state
        .registry
        .ask(GetCampaign(CampaignId::from(Nanoid::from(
            campaign_id.clone(),
        ))))
        .await
    {
        Ok(Some(sup)) => sup,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(InitializeErrorResponse {
                    error: "Campaign not checked out on this shard.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(InitializeErrorResponse {
                    error: "Server is restarting.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
    };

    match supervisor.ask(GetMetadata).await {
        Ok(model) if model.owner_user_id != user.id.to_string() => {
            return StatusCode::FORBIDDEN.into_response();
        }
        Err(kameo::error::SendError::HandlerError(_)) | Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(InitializeErrorResponse {
                    error: "Campaign initialization failed.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
        Ok(_) => {}
    }

    let result = match supervisor
        .ask(InitializeCampaign {
            name: req.name.clone(),
            tagline: req.tagline.clone(),
            game_system: req.game_system.clone(),
            content_locale: req.content_locale.clone(),
        })
        .await
    {
        Ok(result) => result,
        Err(kameo::error::SendError::HandlerError(InitializeCampaignError::AlreadyInitialized)) => {
            return (
                StatusCode::CONFLICT,
                Json(InitializeErrorResponse {
                    error: "Campaign already initialized.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
        Err(kameo::error::SendError::HandlerError(e)) => {
            tracing::error!(error = %e, "initialization failed");
            report_failure(&state, &campaign_id, &e.to_string()).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(InitializeErrorResponse {
                    error: "Campaign initialization failed.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "supervisor unreachable during initialization");
            report_failure(&state, &campaign_id, "supervisor_unreachable").await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(InitializeErrorResponse {
                    error: "Campaign initialization failed.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
    };

    let mirror = MetadataMirrorRequest {
        name: req.name,
        tagline: req.tagline,
        game_system: req.game_system,
        content_locale: req.content_locale,
        wizard_completed_at: result.wizard_completed_at,
    };
    if let Err(e) = state
        .platform_internal
        .report_metadata(&campaign_id, &mirror)
        .await
    {
        tracing::warn!(
            campaign_id = %campaign_id,
            error = %e,
            "platform metadata mirror callback failed; initialization succeeded anyway"
        );
    }

    StatusCode::OK.into_response()
}

async fn report_failure(state: &AppState, campaign_id: &str, reason: &str) {
    if let Err(e) = state
        .platform_internal
        .report_init_failed(campaign_id, reason)
        .await
    {
        tracing::warn!(
            campaign_id = %campaign_id,
            error = %e,
            "platform init-failed callback also failed"
        );
    }
}
