use familiar_systems_campaign::{config::Config, routes::router};
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
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", config.port))
        .await
        .unwrap();
    tracing::info!("campaign listening on :{}", config.port);
    axum::serve(listener, router()).await.unwrap();
}
