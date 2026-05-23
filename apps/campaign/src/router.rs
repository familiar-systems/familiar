use crate::openapi::api_router;
use crate::state::AppState;
use axum::{
    Router, middleware,
    routing::{get, post, put},
};
use familiar_systems_app_shared::middleware::internal_auth::require_internal_bearer;

/// Internal-only routes (`/internal/campaign/*`). Bearer-protected.
/// Not part of the OpenAPI spec (matches the platform pattern).
fn internal_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/internal/campaign",
            post(crate::routes::internal::create_campaign),
        )
        .route(
            "/internal/campaign/{id}/lease",
            put(crate::routes::internal::acquire_lease)
                .delete(crate::routes::internal::release_lease),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            require_internal_bearer,
        ))
        .with_state(state)
}

/// WebSocket route. Not part of OpenAPI (protocol upgrade, not JSON).
fn ws_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/campaign/{id}/ws",
            get(crate::routes::ws::ws_upgrade),
        )
        .with_state(state)
}

/// Composite router: public OpenAPI routes (with state attached) merged
/// with the WebSocket upgrade route and internal bearer-protected routes.
/// Routes use full service-prefixed paths (`/catalog/systems`,
/// `/campaign/{id}`). Reverse proxies strip only the per-environment
/// prefix (nothing in local dev, `/pr-N` in preview) and forward the
/// service prefix intact.
pub fn serve_router(state: AppState) -> Router {
    let (public, _openapi) = api_router().split_for_parts();
    public
        .with_state(state.clone())
        .merge(ws_router(state.clone()))
        .merge(internal_router(state))
}
