pub(crate) mod health;
pub(crate) mod me;

use crate::openapi::api_router;
use crate::state::AppState;
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::{Json, Router, routing::get};
use std::sync::Arc;
use tower_http::{
    cors::{AllowOrigin, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    trace::{DefaultOnResponse, TraceLayer},
};
use tracing::{Level, Span};

pub(crate) fn origin_matches(allowed: &str, origin: &str) -> bool {
    origin == allowed
}

// One span per HTTP request. Declares user_id and session_id as Empty so the
// auth extractor can record() into them; those fields then appear on every
// log event emitted within the request (including the TraceLayer's
// on_response wide event and any handler-emitted events), making the whole
// thing queryable by user or session in the logs.
fn make_request_span(req: &Request) -> Span {
    let request_id = req
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    tracing::info_span!(
        "http_request",
        method = %req.method(),
        uri = %req.uri(),
        request_id = %request_id,
        user_id = tracing::field::Empty,
        session_id = tracing::field::Empty,
    )
}

pub fn router(origins: Vec<String>) -> Router<AppState> {
    // Browser traffic is same-origin under path-based routing (SPA and
    // platform share an apex), so CORS preflights don't fire in practice.
    // The layer stays for any future non-same-origin callers (e.g. a
    // curl-from-a-tool Origin header); CORS_ORIGINS is a simple exact-
    // match allowlist since wildcard subdomains are no longer in use.
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            HeaderName::from_static("authorization"),
            HeaderName::from_static("content-type"),
        ])
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            let Ok(o) = origin.to_str() else { return false };
            origins.iter().any(|allowed| origin_matches(allowed, o))
        }));

    let trace = TraceLayer::new_for_http()
        .make_span_with(make_request_span)
        .on_response(DefaultOnResponse::new().level(Level::INFO));

    // Route paths here are post-strip: they reflect what the platform sees
    // after the reverse proxy (Caddy in dev, Traefik in prod) has removed
    // the /api prefix. /health is reached by browsers at /api/health; /me
    // at /api/me. A route whose path begins with /api will never arrive
    // here; do not add one.
    //
    // Routes are registered through api_router() so the axum dispatcher and
    // the OpenAPI spec stay in lockstep: the same routes! macro that wires
    // a handler into axum also reads its #[utoipa::path] attribute. Drift
    // between "what the server actually serves" and "what the spec says
    // the server serves" becomes a compile error rather than a runtime one.
    let (axum_router, openapi) = api_router().split_for_parts();
    let openapi = Arc::new(openapi);

    let router = axum_router.route(
        "/openapi.json",
        get({
            let openapi = openapi.clone();
            move || {
                let openapi = openapi.clone();
                async move { Json((*openapi).clone()) }
            }
        }),
    );

    // Scalar UI is mounted only when the dev-ui cargo feature is on. It
    // reads the same OpenApi struct served at /openapi.json, so the docs
    // can never disagree with the spec.
    #[cfg(feature = "dev-ui")]
    let router = {
        use utoipa_scalar::{Scalar, Servable};
        router.merge(Scalar::with_url("/docs", (*openapi).clone()))
    };

    // Layer ordering: Axum applies the *last* .layer() outermost. A request
    // travels outermost→innermost, so:
    //   cors -> set_request_id -> trace -> propagate_request_id -> handler
    // set_request_id must precede trace (trace reads the id into the span);
    // propagate must be inside trace so the outgoing response carries the id.
    router
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(trace)
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(cors)
}

#[cfg(test)]
mod tests {
    use super::origin_matches;

    #[test]
    fn exact_match_works() {
        assert!(origin_matches(
            "http://localhost:5173",
            "http://localhost:5173"
        ));
        assert!(!origin_matches(
            "http://localhost:5173",
            "http://localhost:5174"
        ));
        assert!(!origin_matches(
            "http://localhost:5173",
            "https://localhost:5173"
        ));
    }

    #[test]
    fn apex_origin_matches_exactly() {
        let allowed = "https://familiar.systems";
        assert!(origin_matches(allowed, "https://familiar.systems"));
        // Subdomains are not the apex. Under path-based routing the only
        // allowed origin is the apex itself, so cross-subdomain callers
        // are rejected by default.
        assert!(!origin_matches(allowed, "https://app.familiar.systems"));
        assert!(!origin_matches(allowed, "https://evil.familiar.systems"));
        assert!(!origin_matches(allowed, "http://familiar.systems"));
    }

    #[test]
    fn suffix_extension_attacks_blocked() {
        let allowed = "https://familiar.systems";
        assert!(!origin_matches(
            allowed,
            "https://familiar.systems.evil.com"
        ));
    }
}
