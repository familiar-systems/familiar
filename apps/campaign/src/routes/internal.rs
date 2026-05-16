//! `POST /internal/campaign/init`: bearer-protected create-side hook.
//!
//! Asks the [`CampaignRegistry`] to ensure a supervisor exists for the
//! incoming `campaign_id`. The registry serializes ensure calls in its
//! mailbox, so concurrent calls for the same id collapse to one
//! supervisor + one SQLite file. Idempotent: a repeat call with the
//! same id returns 200 against the already-running supervisor.

use crate::actors::registry::EnsureCampaign;
use crate::error::EnsureError;
use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode};
use familiar_systems_app_shared::campaigns::internal::InternalInitRequest;
use kameo::error::SendError;

pub async fn init(
    State(state): State<AppState>,
    Json(req): Json<InternalInitRequest>,
) -> StatusCode {
    let campaign_id_display = req.campaign_id.0.clone();
    // kameo folds the handler's `Result<_, EnsureError>` reply into
    // `SendError::HandlerError(EnsureError)`, so the `ask` future
    // resolves to `Result<ActorRef<_>, SendError<EnsureCampaign,
    // EnsureError>>`. Map each variant to the status the platform's
    // retry logic expects: 503 means "wrong shard, try another"
    // (ShuttingDown, transport failure); 500 means "this campaign
    // genuinely failed to initialize" (Init, SupervisorDied).
    match state
        .registry
        .ask(EnsureCampaign {
            campaign_id: req.campaign_id,
            owner_user_id: req.owner_user_id,
        })
        .await
    {
        Ok(_supervisor_ref) => StatusCode::OK,
        Err(SendError::HandlerError(EnsureError::ShuttingDown)) => {
            tracing::info!(
                campaign_id = %campaign_id_display,
                "rejecting init: registry shutting down"
            );
            StatusCode::SERVICE_UNAVAILABLE
        }
        Err(SendError::HandlerError(handler_err)) => {
            tracing::error!(
                campaign_id = %campaign_id_display,
                error = %handler_err,
                "campaign init failed"
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
