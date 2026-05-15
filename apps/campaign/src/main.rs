use familiar_systems_campaign::{
    clients::platform_internal::PlatformInternalClient, config::Config, routes::serve_router,
    starter_content::Catalog, state::AppState,
};
use std::sync::Arc;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() {
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
    let catalog = Arc::new(
        Catalog::load_from_embedded().expect("starter content failed to parse at startup"),
    );
    let platform_internal =
        PlatformInternalClient::new(config.platform_url.clone(), &config.internal_bearer_primary);
    let state = AppState {
        config: config.clone(),
        catalog,
        platform_internal,
    };
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port))
        .await
        .unwrap();
    tracing::info!("campaign listening on :{}", config.port);
    axum::serve(listener, serve_router(state)).await.unwrap();
}
