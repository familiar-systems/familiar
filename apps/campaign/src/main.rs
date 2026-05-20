use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_campaign::{
    actors::registry::{BeginDrain, CampaignRegistry},
    clients::platform_internal::PlatformInternalClient,
    config::Config,
    db::register_sqlite_vec,
    error::StartupError,
    persistence,
    router::serve_router,
    starter_content::catalog::Catalog,
    state::AppState,
};
use kameo::actor::Spawn;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[tokio::main]
async fn main() -> Result<(), StartupError> {
    init_tracing();
    let config = Arc::new(Config::from_env());
    let validator = Arc::new(HankoSessionValidator::new(config.hanko_api_url.clone()));
    let catalog = Arc::new(Catalog::load_from_embedded().map_err(StartupError::Catalog)?);
    let platform_internal =
        PlatformInternalClient::new(config.platform_url.clone(), &config.internal_bearer_primary);

    // Register sqlite-vec as a process-global auto-extension so every
    // sea-orm pool the registry opens for each campaign gets vec0
    // automatically.
    register_sqlite_vec();

    let store = persistence::store_from_config(&config);
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store,
        config.idle_timeout,
        config.eviction_check_interval,
    ));

    let state = AppState {
        config: config.clone(),
        validator,
        catalog,
        platform_internal,
        registry: registry.clone(),
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|source| StartupError::Bind { addr, source })?;
    tracing::info!(
        port = config.port,
        data_dir = %config.campaign_data_dir.display(),
        idle_timeout_secs = config.idle_timeout.as_secs(),
        "campaign server starting"
    );

    axum::serve(listener, serve_router(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(StartupError::Serve)?;

    tracing::info!("axum drained, beginning drain workflow");
    // The drain workflow runs on its own tokio task, not in the
    // registry's mailbox, so the registry can keep replying
    // `ShuttingDown` to any in-flight queries while child supervisors
    // drain in parallel. `BeginDrain`'s handler returns immediately
    // after spawning the workflow; we wait on the workflow's
    // completion oneshot here.
    let (tx, rx) = oneshot::channel();
    if let Err(e) = registry.tell(BeginDrain { completion: tx }).await {
        tracing::warn!(?e, "BeginDrain send failed; registry may be stopped");
    } else if rx.await.is_err() {
        tracing::warn!(
            "drain completion sender dropped before signalling; registry may have crashed"
        );
    }

    if let Err(e) = registry.stop_gracefully().await {
        tracing::warn!(?e, "registry already stopped");
    }
    // `wait_for_shutdown` returns once the mailbox closes, which can be
    // before `on_stop` has finished running. The `_with_result` variant
    // waits until the lifecycle hook itself has completed.
    registry.wait_for_shutdown_with_result(|_| ()).await;
    tracing::info!("shutdown complete");
    Ok(())
}

fn init_tracing() {
    // Wide-event JSON logging. Span fields are flattened onto the
    // top-level object so `jq '.campaign_id'` works without walking
    // a nested spans array.
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
}

/// Wait for SIGINT or SIGTERM (Unix). Kubernetes sends SIGTERM during
/// graceful pod termination; if we only watched SIGINT we'd hard-kill
/// at the grace-period boundary with dirty actor state.
async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::error!(?e, "failed to install SIGINT handler");
            None
        }
    };
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::error!(?e, "failed to install SIGTERM handler");
            None
        }
    };

    let int = async {
        match sigint.as_mut() {
            Some(s) => {
                s.recv().await;
            }
            None => std::future::pending::<()>().await,
        }
    };
    let term = async {
        match sigterm.as_mut() {
            Some(s) => {
                s.recv().await;
            }
            None => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        _ = int => tracing::info!("SIGINT received"),
        _ = term => tracing::info!("SIGTERM received"),
    }
}
