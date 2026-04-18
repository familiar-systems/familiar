use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{config::Config, routes::router, state::AppState};
use sea_orm::Database;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let config = Arc::new(Config::from_env());
    let db = Database::connect(&config.database_url).await.expect("db connect");
    let validator = Arc::new(HankoSessionValidator::new(config.hanko_api_url.clone()));
    let state = AppState { db, validator, config: config.clone() };
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port)).await.unwrap();
    tracing::info!("platform listening on :{}", config.port);
    axum::serve(listener, router().with_state(state)).await.unwrap();
}
