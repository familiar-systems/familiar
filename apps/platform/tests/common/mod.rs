use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{
    config::Config, migrations::Migrator, routes::router, state::AppState,
};
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use std::sync::Arc;
use wiremock::MockServer;

// Each tests/*.rs is compiled as its own binary; spawn_smoke uses base_url only,
// so hanko + db look unused there. Tasks 15-18 use both.
#[allow(dead_code)]
pub struct TestApp {
    pub base_url: String,
    pub hanko: MockServer,
    pub db: DatabaseConnection,
}

pub async fn spawn_app() -> TestApp {
    let hanko = MockServer::start().await;
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();

    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: hanko.uri(),
        port: 0,
        cors_origins: vec!["http://localhost:5173".into()],
    });
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let state = AppState {
        db: db.clone(),
        validator,
        config,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origins = state.config.cors_origins.clone();
    tokio::spawn(async move {
        axum::serve(listener, router(origins).with_state(state))
            .await
            .unwrap();
    });

    TestApp {
        base_url: format!("http://{addr}"),
        hanko,
        db,
    }
}
