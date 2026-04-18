use axum::{body::Body, http::{Request, StatusCode}};
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{config::Config, routes::router, state::AppState};
use sea_orm::Database;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn health_returns_200() {
    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: "http://127.0.0.1:0".into(),
        port: 0,
        cors_origins: vec![],
    });
    let db = Database::connect(&config.database_url).await.unwrap();
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let state = AppState { db, validator, config };
    let app = router().with_state(state);
    let resp = app
        .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
