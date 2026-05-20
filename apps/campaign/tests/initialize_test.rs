mod common;

use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign::actors::registry::EnsureCampaign;
use serde_json::json;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{header, method, path},
};

fn valid_payload() -> serde_json::Value {
    json!({
        "game_system": "dnd-5e",
        "content_locale": "en",
        "name": "Embergrove Saga",
        "tagline": null,
        "template_slugs": ["common/npc", "common/player"],
        "audio": "opt-out",
        "evals_enabled": false
    })
}

async fn ensure_campaign(app: &common::TestApp, campaign_id: &CampaignId) {
    let _: kameo::actor::ActorRef<
        familiar_systems_campaign::actors::supervisor::CampaignSupervisor,
    > = app
        .registry
        .ask(EnsureCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: UserId::generate(),
        })
        .await
        .expect("ensure campaign");
}

#[tokio::test]
async fn seal_writes_metadata_and_mirrors_to_platform() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    ensure_campaign(&app, &campaign_id).await;

    Mock::given(method("POST"))
        .and(path(format!(
            "/internal/platform/campaigns/{}/metadata",
            campaign_id.0
        )))
        .and(header(
            "authorization",
            format!("Bearer {}", app.bearer).as_str(),
        ))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&app.platform)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/{}/initialize",
            app.base_url, campaign_id.0
        ))
        .json(&valid_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

#[tokio::test]
async fn double_seal_returns_409() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    ensure_campaign(&app, &campaign_id).await;

    Mock::given(method("POST"))
        .and(path(format!(
            "/internal/platform/campaigns/{}/metadata",
            campaign_id.0
        )))
        .respond_with(ResponseTemplate::new(200))
        .mount(&app.platform)
        .await;

    let client = reqwest::Client::new();
    let url = format!("{}/campaign/{}/initialize", app.base_url, campaign_id.0);

    let first = client
        .post(&url)
        .json(&valid_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(first.status().as_u16(), 200);

    let second = client
        .post(&url)
        .json(&valid_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(second.status().as_u16(), 409);
}

#[tokio::test]
async fn initialize_unknown_campaign_returns_404() {
    let app = common::spawn_app().await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/nonexistent-id/initialize",
            app.base_url
        ))
        .json(&valid_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn initialize_rejects_malformed_body_with_4xx() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/test-id/initialize", app.base_url))
        .json(&json!({ "not": "valid" }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_client_error(),
        "expected 4xx for malformed body, got {}",
        resp.status()
    );
}
