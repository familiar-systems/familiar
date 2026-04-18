use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{config::Config, migrations::Migrator, routes::router, state::AppState};
use sea_orm::Database;
use sea_orm_migration::MigratorTrait;
use std::sync::Arc;

#[tokio::test]
async fn boot_migrates_and_serves_health() {
    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: "http://127.0.0.1:0".into(),
        port: 0,
        cors_origins: vec![],
    });
    let db = Database::connect(&config.database_url).await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let state = AppState { db, validator, config };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router(vec![]).with_state(state)).await.unwrap();
    });
    let body = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(body.status().as_u16(), 200);
}
