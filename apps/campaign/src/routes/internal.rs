//! Internal routes (`/internal/campaign/*`): bearer-protected hooks called
//! by the platform tier.
//!
//! Two endpoints:
//!
//! - `POST /internal/campaign`: create a new campaign on this shard.
//! - `PUT  /internal/campaign/{id}/lease`: ensure an existing campaign is
//!   checked out (loaded from disk / object storage).

use crate::actors::registry::{
    CampaignState, CreateCampaign, EnsureCampaign, READY_WAIT_TIMEOUT, ReleaseCampaign, resolve,
};
use crate::error::{CampaignResolveError, EnsureError};
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

/// Await a just-initiated checkout to a terminal state and map it to a status,
/// preserving the previous synchronous-ensure contract for the platform:
/// ready -> 200, load failure (init error / supervisor died) -> 500, drain or
/// timeout -> 503. The checkout itself runs off the registry's mailbox.
async fn await_checkout(
    state: CampaignState,
    campaign_id: impl std::fmt::Display,
    op: &str,
) -> StatusCode {
    match resolve(Some(state), READY_WAIT_TIMEOUT).await {
        Ok(_handle) => StatusCode::OK,
        Err(CampaignResolveError::LoadFailed) => {
            tracing::error!(campaign_id = %campaign_id, op = %op, "checkout load failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
        Err(e) => {
            tracing::warn!(campaign_id = %campaign_id, op = %op, reason = %e, "checkout not ready");
            e.status()
        }
    }
}

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
        Ok(state) => await_checkout(state, &campaign_id_display, "create").await,
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
        Ok(state) => await_checkout(state, &campaign_id, "lease").await,
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
