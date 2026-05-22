use crate::actors::registry::{GetPhase, Phase};
use crate::state::AppState;
use axum::{extract::State, http::StatusCode, response::IntoResponse};

/// Readiness-style health endpoint. Returns 200 with `ready` body while
/// the registry is in `Phase::Ready`; once drain has begun, returns 503
/// with `draining`. k8s readiness probes consume the status code to
/// take the pod out of the LB rotation immediately when drain starts,
/// so the platform's `/internal/campaign/init` calls stop being routed
/// here before in-flight requests finish. If the registry itself is
/// unreachable (mailbox closed or actor crashed), returns 503 with
/// `registry_unavailable`.
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = OK, description = "Ready"),
        (status = SERVICE_UNAVAILABLE, description = "Draining or unavailable"),
    ),
)]
pub async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.ask(GetPhase).await {
        Ok(Phase::Ready) => (StatusCode::OK, "ready"),
        Ok(Phase::Draining) => (StatusCode::SERVICE_UNAVAILABLE, "draining"),
        Err(send_err) => {
            tracing::warn!(?send_err, "/health: registry unreachable");
            (StatusCode::SERVICE_UNAVAILABLE, "registry_unavailable")
        }
    }
}
