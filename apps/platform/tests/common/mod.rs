use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_platform::{
    clients::campaign_internal::CampaignInternalClient, config::Config, migrations::Migrator,
    routes::serve_router, state::AppState,
};
use sea_orm::{Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;
use std::sync::{Arc, Once};
use wiremock::MockServer;

/// Initialize tracing once per test binary so `tracing::error!` events show up
/// when running with `--nocapture`. Rerun-safe via `Once`.
fn init_tracing_for_tests() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
            )
            .with_test_writer()
            .try_init();
    });
}

// Each tests/*.rs is compiled as its own binary; spawn_smoke uses base_url only,
// auth_test uses hanko, db is exposed for tests that assert state.
#[allow(dead_code)]
pub struct TestApp {
    pub base_url: String,
    pub hanko: MockServer,
    pub campaign: MockServer,
    pub db: DatabaseConnection,
    pub bearer: String,
}

pub async fn spawn_app() -> TestApp {
    init_tracing_for_tests();
    let hanko = MockServer::start().await;
    // Standalone wiremock server so route tests can assert that the platform
    // dispatched the expected `POST /internal/campaign/init` to the campaign
    // tier. Tests that don't care can ignore it; an unmounted endpoint
    // returns 404, which exercises the failure path naturally.
    let campaign = MockServer::start().await;
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();

    let bearer = "test-internal-bearer".to_string();
    let config = Arc::new(Config {
        database_url: "sqlite::memory:".into(),
        hanko_api_url: hanko.uri(),
        port: 0,
        cors_origins: vec!["http://localhost:5173".into()],
        internal_bearer_primary: bearer.clone(),
        internal_bearer_secondary: None,
        campaign_shard_url: campaign.uri(),
    });
    let validator = Arc::new(HankoSessionValidator::new(&config.hanko_api_url));
    let campaign_internal = CampaignInternalClient::new(campaign.uri(), &bearer);
    let state = AppState {
        db: db.clone(),
        validator,
        config,
        campaign_internal,
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let origins = state.config.cors_origins.clone();
    tokio::spawn(async move {
        axum::serve(listener, serve_router(state, origins))
            .await
            .unwrap();
    });

    TestApp {
        base_url: format!("http://{addr}"),
        hanko,
        campaign,
        db,
        bearer,
    }
}

/// Mounts a Hanko `/sessions/validate` mock that accepts every Bearer call
/// and resolves it to `(subject, email)`. Tests that need an authenticated
/// SPA call use this to skip the inline mock boilerplate.
#[allow(dead_code)]
pub async fn mock_hanko_user(app: &TestApp, subject: &str, email: &str) {
    use wiremock::{
        Mock, ResponseTemplate,
        matchers::{method, path},
    };
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": subject,
                "email": {"address": email, "is_primary": true, "is_verified": true},
                "expiration": "2099-01-01T00:00:00Z",
                "session_id": "s"
            }
        })))
        .mount(&app.hanko)
        .await;
}
