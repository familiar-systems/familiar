pub mod catalog;
pub mod initialize;
pub mod internal;

use crate::actors::registry::{GetPhase, Phase};
use crate::middleware::internal_auth::require_internal_bearer;
use crate::state::AppState;
use axum::{
    Router,
    extract::State,
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
};

/// Public routes (catalog + per-campaign endpoints). Routes use full
/// service-prefixed paths (`/catalog/systems`, `/campaign/<id>/initialize`).
/// Reverse proxies strip only the per-environment prefix (nothing in local
/// dev, `/pr-N` in preview) and forward the service prefix intact.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/catalog/systems", get(catalog::list_systems))
        .route("/campaign/{id}/initialize", post(initialize::initialize))
}

/// Internal-only routes (`/internal/campaign/*`). Bearer-protected.
pub fn internal_router(state: AppState) -> Router {
    Router::new()
        .route("/internal/campaign/init", post(internal::init))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_internal_bearer,
        ))
        .with_state(state)
}

/// Composite router: public (with state attached) merged with internal.
pub fn serve_router(state: AppState) -> Router {
    public_router()
        .with_state(state.clone())
        .merge(internal_router(state))
}

/// Readiness-style health endpoint. Returns 200 with `ready` body while
/// the registry is in `Phase::Ready`; once drain has begun, returns 503
/// with `draining`. k8s readiness probes consume the status code to
/// take the pod out of the LB rotation immediately when drain starts,
/// so the platform's `/internal/campaign/init` calls stop being routed
/// here before in-flight requests finish. If the registry itself is
/// unreachable (mailbox closed or actor crashed), returns 503 with
/// `registry_unavailable`.
async fn health(State(state): State<AppState>) -> impl IntoResponse {
    match state.registry.ask(GetPhase).await {
        Ok(Phase::Ready) => (StatusCode::OK, "ready"),
        Ok(Phase::Draining) => (StatusCode::SERVICE_UNAVAILABLE, "draining"),
        Err(send_err) => {
            tracing::warn!(?send_err, "/health: registry unreachable");
            (StatusCode::SERVICE_UNAVAILABLE, "registry_unavailable")
        }
    }
}
