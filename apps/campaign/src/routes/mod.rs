pub mod catalog;
pub mod initialize;
pub mod internal;

use crate::middleware::internal_auth::require_internal_bearer;
use crate::state::AppState;
use axum::{
    Router, middleware,
    routing::{get, post},
};

/// Public routes (catalog + per-campaign initialize). Caddy strips
/// `/campaign/` and `/catalog/` before requests arrive, so paths here are
/// post-strip: `/systems`, `/<id>/initialize`. The `/catalog/*` and
/// `/campaign/*` Caddy entries route to the same binary at port 3001;
/// merging here keeps the per-binary axum routing flat.
pub fn public_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/systems", get(catalog::list_systems))
        .route("/{id}/initialize", post(initialize::initialize))
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

async fn health() -> &'static str {
    "ok"
}
