//! `GET /campaign/{id}` and `PATCH /campaign/{id}` -- campaign metadata.

use crate::actors::database_writer::{GetMetadata, MetadataError, PatchCampaignError};
use crate::actors::registry::GetCampaign;
use crate::actors::supervisor::PatchCampaignMetadata;
use crate::middleware::auth::AuthenticatedUser;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use familiar_systems_app_shared::campaigns::internal::PatchCampaignMirror;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign_shared::id::ThingId;
use familiar_systems_campaign_shared::onboarding::initialize::{
    CampaignErrorResponse, PatchCampaignRequest,
};
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
        // 4XX
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Not the campaign owner"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        // 5XX
        (status = SERVICE_UNAVAILABLE, description = "Server restarting"),
        (status = INTERNAL_SERVER_ERROR, description = "Internal error"),
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

    let caller = UserId(user.id);
    match supervisor.ask(GetMetadata).await {
        Ok(model) => {
            if !model.is_owned_by(&caller) {
                return StatusCode::FORBIDDEN.into_response();
            }
            let resp = CampaignMetadataResponse {
                campaign_id,
                name: model.name,
                tagline: model.tagline,
                game_system: model.game_system,
                content_locale: model.content_locale,
                home_thing_id: model.home_thing_id.map(ThingId::from),
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

#[utoipa::path(
    patch,
    path = "/campaign/{id}",
    tag = "campaign",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    request_body = PatchCampaignRequest,
    responses(
        (status = OK, description = "Campaign updated", body = CampaignMetadataResponse),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Not the campaign owner"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = 409, description = "Wizard already completed"),
        (status = 422, description = "Required fields missing for wizard completion"),
        (status = 500, description = "Update failed", body = CampaignErrorResponse),
    ),
)]
pub async fn patch_campaign(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<PatchCampaignRequest>,
) -> impl IntoResponse {
    tracing::info!(
        campaign_id = %campaign_id,
        wizard_complete = req.wizard_complete,
        "patching campaign metadata"
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
                Json(CampaignErrorResponse {
                    error: "Campaign not checked out on this shard.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
        Err(_) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(CampaignErrorResponse {
                    error: "Server is restarting.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
    };

    let caller = UserId(user.id);
    let existing = match supervisor.ask(GetMetadata).await {
        Ok(model) if !model.is_owned_by(&caller) => {
            return StatusCode::FORBIDDEN.into_response();
        }
        Ok(model) => model,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CampaignErrorResponse {
                    error: "Failed to read campaign metadata.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
    };

    if req.wizard_complete {
        let name_present = req.name.is_some() || existing.name != "Untitled";
        let system_present = req.game_system.is_some() || existing.game_system.is_some();
        let locale_present = req.content_locale.is_some() || existing.content_locale.is_some();
        if !name_present || !system_present || !locale_present {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(CampaignErrorResponse {
                    error: "Required fields missing: name, game_system, and content_locale must be set before completing the wizard.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
    }

    let result = match supervisor
        .ask(PatchCampaignMetadata {
            name: req.name.clone(),
            tagline: req.tagline.clone(),
            game_system: req.game_system.clone(),
            content_locale: req.content_locale.clone(),
            complete_wizard: req.wizard_complete,
        })
        .await
    {
        Ok(result) => result,
        Err(kameo::error::SendError::HandlerError(PatchCampaignError::AlreadyInitialized)) => {
            return (
                StatusCode::CONFLICT,
                Json(CampaignErrorResponse {
                    error: "Campaign wizard already completed.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
        Err(kameo::error::SendError::HandlerError(e)) => {
            tracing::error!(error = %e, "patch failed");
            report_failure(&state, &campaign_id, &e.to_string()).await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CampaignErrorResponse {
                    error: "Campaign update failed.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!(error = %e, "supervisor unreachable during patch");
            report_failure(&state, &campaign_id, "supervisor_unreachable").await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CampaignErrorResponse {
                    error: "Campaign update failed.".to_string(),
                    campaign_id,
                }),
            )
                .into_response();
        }
    };

    let mirror = PatchCampaignMirror {
        name: req.name,
        tagline: req.tagline,
        game_system: req.game_system,
        content_locale: req.content_locale,
        wizard_completed_at: if result.wizard_just_completed {
            result.model.wizard_completed_at
        } else {
            None
        },
    };
    if let Err(e) = state
        .platform_internal
        .patch_campaign(&campaign_id, &mirror)
        .await
    {
        tracing::warn!(
            campaign_id = %campaign_id,
            error = %e,
            "platform metadata mirror failed; update succeeded anyway"
        );
    }

    let model = result.model;
    let resp = CampaignMetadataResponse {
        campaign_id,
        name: model.name,
        tagline: model.tagline,
        game_system: model.game_system,
        content_locale: model.content_locale,
        home_thing_id: model.home_thing_id.map(ThingId::from),
        wizard_completed_at: model.wizard_completed_at.map(|dt| dt.to_rfc3339()),
        created_at: model.created_at.to_rfc3339(),
        updated_at: model.updated_at.to_rfc3339(),
    };

    (StatusCode::OK, Json(resp)).into_response()
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
