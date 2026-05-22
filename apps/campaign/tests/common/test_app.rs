use familiar_systems_app_shared::auth::HankoSessionValidator;
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

use familiar_systems_app_shared::id::UserId;
use fs_id::Uuid;

pub const TEST_USER_SUBJECT: &str = "0195b4a0-0000-7000-8000-000000000001";
pub const TEST_SESSION_TOKEN: &str = "test-session-token";

#[allow(dead_code)]
pub fn test_user_id() -> UserId {
    UserId(Uuid::parse_str(TEST_USER_SUBJECT).expect("valid test UUID"))
}

#[allow(dead_code)]
pub struct TestApp {
    pub base_url: String,
    pub platform: MockServer,
    pub hanko: MockServer,
    pub bearer: String,
    pub data_dir: TempDir,
    pub registry: ActorRef<CampaignRegistry>,
}

#[allow(dead_code)]
impl TestApp {
    pub fn auth_header(&self) -> String {
        format!("Bearer {TEST_SESSION_TOKEN}")
    }
}

#[allow(dead_code)]
pub async fn spawn_app() -> TestApp {
    register_sqlite_vec();
    let platform = MockServer::start().await;
    let hanko = MockServer::start().await;

    // Mount a default Hanko mock that accepts any session token.
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/sessions/validate"))
        .respond_with(wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": TEST_USER_SUBJECT,
                "email": {"address": "test@example.com", "is_primary": true, "is_verified": true},
                "expiration": "2099-01-01T00:00:00Z",
                "session_id": "test-session-1"
            }
        })))
        .mount(&hanko)
        .await;

    let bearer = "test-internal-bearer".to_string();
    let data_dir = TempDir::new().expect("create tempdir for campaign data");
    let config = Arc::new(Config {
        storage_backend: StorageBackend::Local,
        s3: None,
        port: 0,
        hanko_api_url: hanko.uri(),
        campaign_data_dir: data_dir.path().to_path_buf(),
        internal_bearer_primary: bearer.clone(),
        internal_bearer_secondary: None,
        platform_url: platform.uri(),
        idle_timeout: Duration::from_secs(300),
        eviction_check_interval: Duration::from_secs(60),
        heartbeat_interval: Duration::from_secs(300),
    });
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let catalog =
        Arc::new(Catalog::load_from_embedded().expect("embedded catalog should parse in tests"));
    let platform_internal = PlatformInternalClient::new(platform.uri(), &bearer);
    let store: Arc<dyn familiar_systems_campaign::persistence::CampaignStore> =
        Arc::new(LocalCampaignStore::new(data_dir.path().to_path_buf()));
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store,
        config.idle_timeout,
        config.eviction_check_interval,
        Some(platform_internal.clone()),
    ));
    let state = AppState {
        config,
        validator,
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
        hanko,
        bearer,
        data_dir,
        registry,
    }
}
