use familiar_systems_campaign::{
    clients::platform_internal::PlatformInternalClient, config::Config, routes::serve_router,
    starter_content::Catalog, state::AppState,
};
use std::sync::Arc;
use wiremock::MockServer;

#[allow(dead_code)]
pub struct TestApp {
    pub base_url: String,
    pub platform: MockServer,
    pub bearer: String,
}

#[allow(dead_code)]
pub async fn spawn_app() -> TestApp {
    // The campaign tier calls the platform on the deliberate-fail path, so
    // tests need a stand-in platform to assert the callback lands. wiremock
    // gives one with verifiable expectations per test.
    let platform = MockServer::start().await;
    let bearer = "test-internal-bearer".to_string();
    let config = Arc::new(Config {
        port: 0,
        campaign_data_dir: std::path::PathBuf::from("/tmp/test-campaign-data"),
        internal_bearer_primary: bearer.clone(),
        internal_bearer_secondary: None,
        platform_url: platform.uri(),
    });
    let catalog =
        Arc::new(Catalog::load_from_embedded().expect("embedded catalog should parse in tests"));
    let platform_internal = PlatformInternalClient::new(platform.uri(), &bearer);
    let state = AppState {
        config,
        catalog,
        platform_internal,
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
    }
}
