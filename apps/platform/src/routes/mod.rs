mod health;
mod me;

use crate::state::AppState;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::{Router, routing::get};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub(crate) fn origin_matches(allowed: &str, origin: &str) -> bool {
    origin == allowed
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
    Router::new()
        .route("/health", get(health::health))
        .route("/me", get(me::me))
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
