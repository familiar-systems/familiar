use crate::middleware::internal_auth::require_internal_bearer;
use crate::openapi::api_router;
use crate::state::AppState;
use axum::{Router, middleware, routing::post};

/// Internal-only routes (`/internal/campaign/*`). Bearer-protected.
/// Not part of the OpenAPI spec (matches the platform pattern).
fn internal_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/internal/campaign/init",
            post(crate::routes::internal::init),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_internal_bearer,
        ))
        .with_state(state)
}

/// Composite router: public OpenAPI routes (with state attached) merged
/// with internal bearer-protected routes. Routes use full service-prefixed
/// paths (`/catalog/systems`, `/campaign/<id>/initialize`). Reverse proxies
/// strip only the per-environment prefix (nothing in local dev, `/pr-N` in
/// preview) and forward the service prefix intact.
pub fn serve_router(state: AppState) -> Router {
    let (public, _openapi) = api_router().split_for_parts();
    public
        .with_state(state.clone())
        .merge(internal_router(state))
}
