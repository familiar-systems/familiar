mod health;
mod me;

use crate::state::AppState;
use axum::http::{HeaderName, HeaderValue, Method};
use axum::{routing::get, Router};
use tower_http::cors::{AllowOrigin, CorsLayer};

pub fn router(origins: Vec<String>) -> Router<AppState> {
    let cors = CorsLayer::new()
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            HeaderName::from_static("authorization"),
            HeaderName::from_static("content-type"),
        ])
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            let Ok(o) = origin.to_str() else { return false };
            origins.iter().any(|allowed| {
                if let Some(suffix) = allowed.strip_prefix("https://*.") {
                    // Wildcard: match https://<subdomain>.<suffix>
                    o.strip_prefix("https://").is_some_and(|rest| {
                        rest == suffix || rest.ends_with(&format!(".{suffix}"))
                    })
                } else {
                    o == allowed
                }
            })
        }));
    Router::new()
        .route("/health", get(health::health))
        .route("/me", get(me::me))
        .layer(cors)
}
