//! `POST /internal/campaign/init` — bearer-protected create-side hook.
//!
//! v0: no-op. The next slice replaces this handler with per-campaign DB
//! provisioning; today the route exists so the platform's create flow has
//! a real target to call and the bearer plumbing is exercised end-to-end.

use crate::state::AppState;
use axum::{Json, extract::State, http::StatusCode};
use familiar_systems_app_shared::campaigns::internal::InternalInitRequest;

pub async fn init(
    State(_state): State<AppState>,
    Json(req): Json<InternalInitRequest>,
) -> StatusCode {
    tracing::info!(
        campaign_id = %req.campaign_id.0,
        "received internal init (no-op in v0)"
    );
    StatusCode::OK
}
