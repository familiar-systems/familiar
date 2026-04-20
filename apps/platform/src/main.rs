use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{
    config::Config, migrations::Migrator, routes::router, state::AppState,
};
use sea_orm::Database;
use sea_orm_migration::MigratorTrait;
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() {
    // Wide-event logging: one JSON object per log event, with span fields
    // (request_id, user_id, session_id) flattened onto the top level so
    // `jq '.request_id'` works without walking a nested `spans` array.
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .with(
            fmt::layer()
                .json()
                .flatten_event(true)
                .with_current_span(true)
                .with_span_list(false),
        )
        .init();
    let config = Arc::new(Config::from_env());
    let db = Database::connect(&config.database_url)
        .await
        .expect("db connect");
    Migrator::up(&db, None).await.expect("migrate");
    let validator = Arc::new(HankoSessionValidator::new(config.hanko_api_url.clone()));
    let state = AppState {
        db,
        validator,
        config: config.clone(),
    };
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port))
        .await
        .unwrap();
    tracing::info!("platform listening on :{}", config.port);
    axum::serve(
        listener,
        router(config.cors_origins.clone()).with_state(state),
    )
    .await
    .unwrap();
}
