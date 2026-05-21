//! Internal routes (`/internal/campaign/*`): bearer-protected hooks called
//! by the platform tier.
//!
//! Two endpoints:
//!
//! - `POST /internal/campaign`: create a new campaign on this shard.
//! - `PUT  /internal/campaign/{id}/lease`: ensure an existing campaign is
//!   checked out (loaded from disk / object storage).

use crate::actors::registry::{CreateCampaign, EnsureCampaign, ReleaseCampaign};
use crate::error::EnsureError;
use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use familiar_systems_app_shared::campaigns::internal::CreateCampaignRequest;
use familiar_systems_app_shared::id::CampaignId;
use fs_id::Nanoid;
use kameo::error::SendError;

/// `POST /internal/campaign`: create a new campaign with the given owner.
/// Idempotent on `campaign_id`.
pub async fn create_campaign(
    State(state): State<AppState>,
    Json(req): Json<CreateCampaignRequest>,
) -> StatusCode {
    let campaign_id_display = req.campaign_id.0.clone();
    match state
        .registry
        .ask(CreateCampaign {
            campaign_id: req.campaign_id,
            owner_user_id: req.owner_user_id,
        })
        .await
    {
        Ok(_supervisor_ref) => StatusCode::OK,
        Err(SendError::HandlerError(EnsureError::ShuttingDown)) => {
            tracing::info!(
                campaign_id = %campaign_id_display,
                "rejecting create: registry shutting down"
            );
            StatusCode::SERVICE_UNAVAILABLE
        }
        Err(SendError::HandlerError(handler_err)) => {
            tracing::error!(
                campaign_id = %campaign_id_display,
                error = %handler_err,
                "campaign create failed"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        }
        Err(transport_err) => {
            tracing::error!(
                campaign_id = %campaign_id_display,
                error = ?transport_err,
                "registry unreachable"
            );
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

/// `PUT /internal/campaign/{id}/lease`: ensure an existing campaign is
/// checked out on this shard. No body needed; the campaign must already
/// exist (created via `POST /internal/campaign` or present in object
/// storage). Idempotent.
pub async fn acquire_lease(
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
) -> StatusCode {
    match state
        .registry
        .ask(EnsureCampaign {
            campaign_id: CampaignId::from(Nanoid::from(campaign_id.clone())),
        })
        .await
    {
        Ok(_supervisor_ref) => StatusCode::OK,
        Err(SendError::HandlerError(EnsureError::ShuttingDown)) => {
            tracing::info!(
                campaign_id = %campaign_id,
                "rejecting lease: registry shutting down"
            );
            StatusCode::SERVICE_UNAVAILABLE
        }
        Err(SendError::HandlerError(handler_err)) => {
            tracing::error!(
                campaign_id = %campaign_id,
                error = %handler_err,
                "lease acquisition failed"
            );
            StatusCode::INTERNAL_SERVER_ERROR
        }
        Err(transport_err) => {
            tracing::error!(
                campaign_id = %campaign_id,
                error = ?transport_err,
                "registry unreachable"
            );
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}

/// `DELETE /internal/campaign/{id}/lease`: release a campaign from this
/// shard. The platform calls this to proactively evict a campaign. If the
/// campaign is not loaded, returns 200 (idempotent).
pub async fn release_lease(
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
) -> StatusCode {
    match state
        .registry
        .ask(ReleaseCampaign {
            campaign_id: CampaignId::from(Nanoid::from(campaign_id.clone())),
        })
        .await
    {
        Ok(()) => StatusCode::OK,
        Err(transport_err) => {
            tracing::error!(
                campaign_id = %campaign_id,
                error = ?transport_err,
                "registry unreachable during release"
            );
            StatusCode::SERVICE_UNAVAILABLE
        }
    }
}
