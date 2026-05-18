use familiar_systems_campaign::{
    actors::registry::CampaignRegistry, clients::platform_internal::PlatformInternalClient,
    config::Config, db::register_sqlite_vec, routes::serve_router, starter_content::Catalog,
    state::AppState,
};
use kameo::actor::{ActorRef, Spawn};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use wiremock::MockServer;

#[allow(dead_code)]
pub struct TestApp {
    pub base_url: String,
    pub platform: MockServer,
    pub bearer: String,
    pub data_dir: TempDir,
    /// Live registry handle so tests can drive lifecycle (e.g. begin
    /// drain to assert the internal routes flip to 503).
    pub registry: ActorRef<CampaignRegistry>,
}

#[allow(dead_code)]
pub async fn spawn_app() -> TestApp {
    // sqlite-vec registration is global and idempotent; safe to call
    // once per test run even if multiple TestApps spawn.
    register_sqlite_vec();
    // The campaign tier calls the platform on the deliberate-fail path, so
    // tests need a stand-in platform to assert the callback lands. wiremock
    // gives one with verifiable expectations per test.
    let platform = MockServer::start().await;
    let bearer = "test-internal-bearer".to_string();
    let data_dir = TempDir::new().expect("create tempdir for campaign data");
    let config = Arc::new(Config {
        port: 0,
        campaign_data_dir: data_dir.path().to_path_buf(),
        internal_bearer_primary: bearer.clone(),
        internal_bearer_secondary: None,
        platform_url: platform.uri(),
        // Long idle timeout so the supervisor doesn't evict mid-test.
        idle_timeout: Duration::from_secs(300),
        eviction_check_interval: Duration::from_secs(60),
    });
    let catalog =
        Arc::new(Catalog::load_from_embedded().expect("embedded catalog should parse in tests"));
    let platform_internal = PlatformInternalClient::new(platform.uri(), &bearer);
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        config.campaign_data_dir.clone(),
        config.idle_timeout,
        config.eviction_check_interval,
    ));
    let state = AppState {
        config,
        catalog,
        platform_internal,
        registry: registry.clone(),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, serve_router(state)).await.unwrap();
    });

    TestApp {
        base_url: format!("http://{addr}"),
        platform,
        bearer,
        data_dir,
        registry,
    }
}
