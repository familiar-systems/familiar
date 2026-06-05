use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{
    clients::campaign_internal::CampaignInternalClient, config::Config, db, routes::serve_router,
    state::AppState,
};
use std::sync::Arc;
use tower::ServiceExt;

async fn make_app() -> axum::Router {
    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: "http://127.0.0.1:0".into(),
        port: 0,
        cors_origins: vec![],
        internal_bearer_primary: "health-test-bearer".into(),
        internal_bearer_secondary: None,
        campaign_shard_url: "http://127.0.0.1:0".into(),
    });
    let db = db::connect(&config.database_url).await.unwrap();
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let campaign_internal = CampaignInternalClient::new(
        config.campaign_shard_url.clone(),
        &config.internal_bearer_primary,
    );
    let state = AppState {
        db,
        validator,
        config,
        campaign_internal,
        loaded_cache: Default::default(),
    };
    serve_router(state, vec![])
}

#[tokio::test]
async fn health_returns_200() {
    let app = make_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn response_carries_x_request_id_header() {
    let app = make_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // SetRequestIdLayer stamps a UUID on every request; PropagateRequestIdLayer
    // copies it to the response. Downstream logs correlate by this header.
    let id = resp
        .headers()
        .get("x-request-id")
        .expect("x-request-id must be propagated to the response")
        .to_str()
        .unwrap();
    assert!(
        uuid::Uuid::parse_str(id).is_ok(),
        "x-request-id must be a UUID, got {id}"
    );
}
