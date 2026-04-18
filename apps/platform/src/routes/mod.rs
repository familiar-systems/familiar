mod health;
mod me;

use crate::state::AppState;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::{Router, routing::get};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub(crate) fn origin_matches(allowed: &str, origin: &str) -> bool {
    if let Some(suffix) = allowed.strip_prefix("https://*.") {
        origin
            .strip_prefix("https://")
            .is_some_and(|rest| rest == suffix || rest.ends_with(&format!(".{suffix}")))
    } else {
        origin == allowed
    }
}

pub fn router(origins: Vec<String>) -> Router<AppState> {
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
    fn wildcard_matches_subdomain() {
        let allowed = "https://*.preview.familiar.systems";
        assert!(origin_matches(
            allowed,
            "https://app-pr-1.preview.familiar.systems"
        ));
        assert!(origin_matches(
            allowed,
            "https://api-pr-1.preview.familiar.systems"
        ));
        assert!(origin_matches(
            allowed,
            "https://multi.level.preview.familiar.systems"
        ));
    }

    #[test]
    fn wildcard_matches_bare_suffix() {
        let allowed = "https://*.preview.familiar.systems";
        assert!(origin_matches(allowed, "https://preview.familiar.systems"));
    }

    #[test]
    fn wildcard_rejects_http_scheme() {
        let allowed = "https://*.preview.familiar.systems";
        assert!(!origin_matches(
            allowed,
            "http://app-pr-1.preview.familiar.systems"
        ));
    }

    #[test]
    fn wildcard_rejects_suffix_extension_attack() {
        // Prevent: attacker registers preview.familiar.systems.evil.com and tries to spoof.
        let allowed = "https://*.preview.familiar.systems";
        assert!(!origin_matches(
            allowed,
            "https://preview.familiar.systems.evil.com"
        ));
        assert!(!origin_matches(
            allowed,
            "https://app.preview.familiar.systems.evil.com"
        ));
    }

    #[test]
    fn wildcard_rejects_unrelated_domain() {
        let allowed = "https://*.preview.familiar.systems";
        assert!(!origin_matches(allowed, "https://evil.com"));
        assert!(!origin_matches(allowed, "https://familiar.systems"));
    }

    #[test]
    fn wildcard_rejects_prefix_match_only() {
        // The leftmost dot is the boundary. "preview.familiar.systems" must be the suffix
        // following a dot, not a substring.
        let allowed = "https://*.preview.familiar.systems";
        assert!(!origin_matches(
            allowed,
            "https://xpreview.familiar.systems"
        ));
    }
}
