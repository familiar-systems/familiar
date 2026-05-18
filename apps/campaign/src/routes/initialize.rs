//! `POST /campaign/<id>/initialize` - wizard's Seal handler.
//!
//! v0 thin slice: validates the payload shape, fires a callback to the
//! platform's `init-failed` endpoint (exercising the bidirectional
//! internal-API plumbing), and returns 500 with a structured body the SPA
//! renders inline. The next slice replaces the deliberate-fail block with
//! the real init transaction; the failure path stays as the
//! "init-actually-broke" channel.

use crate::state::AppState;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use familiar_systems_campaign_shared::onboarding::initialize::{
    InitializeErrorResponse, InitializeRequest,
};

const FAILURE_REASON: &str = "deliberate_thin_slice_failure";
const PUBLIC_ERROR: &str = "Campaign initialization is not yet wired up. \
                            This is a known thin-slice failure.";

pub async fn initialize(
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<InitializeRequest>,
) -> impl IntoResponse {
    // Light validation: empty name or unknown game_system would surface as
    // 400 in the next slice's real handler. Today we just log so the dev
    // logs show that the request was structurally well-formed before we
    // deliberately fail.
    tracing::info!(
        campaign_id = %campaign_id,
        game_system = %req.game_system,
        content_locale = %req.content_locale,
        template_count = req.template_slugs.len(),
        audio = ?req.audio,
        evals_enabled = req.evals_enabled,
        "deliberate thin-slice failure on initialize"
    );

    // Fire the platform callback. Awaited so the platform records the
    // failure before the SPA learns of it; if the callback itself fails,
    // log at warn but still return the deliberate failure to the SPA.
    // Masking the FE-visible failure with a different one defeats the
    // point of this thin slice.
    if let Err(e) = state
        .platform_internal
        .report_init_failed(&campaign_id, FAILURE_REASON)
        .await
    {
        tracing::warn!(
            campaign_id = %campaign_id,
            error = %e,
            "platform init-failed callback failed; returning thin-slice 500 anyway"
        );
    }

    let body = InitializeErrorResponse {
        error: PUBLIC_ERROR.to_string(),
        campaign_id,
    };
    (StatusCode::INTERNAL_SERVER_ERROR, Json(body))
}
