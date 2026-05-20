use familiar_systems_campaign::{
    actors::registry::CampaignRegistry,
    clients::platform_internal::PlatformInternalClient,
    config::{Config, StorageBackend},
    db::register_sqlite_vec,
    persistence::LocalCampaignStore,
    router::serve_router,
    starter_content::Catalog,
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
    pub registry: ActorRef<CampaignRegistry>,
}

#[allow(dead_code)]
pub async fn spawn_app() -> TestApp {
    register_sqlite_vec();
    let platform = MockServer::start().await;
    let bearer = "test-internal-bearer".to_string();
    let data_dir = TempDir::new().expect("create tempdir for campaign data");
    let config = Arc::new(Config {
        storage_backend: StorageBackend::Local,
        port: 0,
        campaign_data_dir: data_dir.path().to_path_buf(),
        internal_bearer_primary: bearer.clone(),
        internal_bearer_secondary: None,
        platform_url: platform.uri(),
        idle_timeout: Duration::from_secs(300),
        eviction_check_interval: Duration::from_secs(60),
    });
    let catalog =
        Arc::new(Catalog::load_from_embedded().expect("embedded catalog should parse in tests"));
    let platform_internal = PlatformInternalClient::new(platform.uri(), &bearer);
    let store: Arc<dyn familiar_systems_campaign::persistence::CampaignStore> =
        Arc::new(LocalCampaignStore::new(data_dir.path().to_path_buf()));
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store,
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
