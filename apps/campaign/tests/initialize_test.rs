mod common;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::registry::{CreateCampaign, resolve};
use familiar_systems_campaign_shared::onboarding::metadata::CampaignMetadataResponse;
use serde_json::json;
use std::time::Duration;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{header, method, path},
};

fn wizard_payload() -> serde_json::Value {
    json!({
        "game_system": "dnd-5e",
        "content_locale": "en",
        "name": "Embergrove Saga",
        "tagline": null,
        "template_slugs": ["common/npc", "common/player"],
        "audio": "opt-out",
        "evals_enabled": false,
        "wizard_complete": true
    })
}

async fn create_campaign(app: &common::TestApp, campaign_id: &CampaignId) {
    let state = app
        .registry
        .ask(CreateCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: common::test_user_id(),
        })
        .await
        .expect("create campaign");
    resolve(Some(state), Duration::from_secs(30))
        .await
        .expect("campaign ready");
}

#[tokio::test]
async fn patch_with_wizard_complete_writes_metadata_and_mirrors() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    Mock::given(method("PATCH"))
        .and(path(format!(
            "/internal/platform/campaign/{}",
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
        .patch(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&wizard_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: CampaignMetadataResponse = resp.json().await.unwrap();
    assert_eq!(body.name, "Embergrove Saga");
    assert!(body.wizard_completed_at.is_some());
}

#[tokio::test]
async fn double_wizard_complete_returns_409() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    Mock::given(method("PATCH"))
        .and(path(format!(
            "/internal/platform/campaign/{}",
            campaign_id.0
        )))
        .respond_with(ResponseTemplate::new(200))
        .mount(&app.platform)
        .await;

    let client = reqwest::Client::new();
    let url = format!("{}/campaign/{}", app.base_url, campaign_id.0);

    let first = client
        .patch(&url)
        .header("authorization", app.auth_header())
        .json(&wizard_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(first.status().as_u16(), 200);

    let second = client
        .patch(&url)
        .header("authorization", app.auth_header())
        .json(&wizard_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(second.status().as_u16(), 409);
}

#[tokio::test]
async fn patch_unknown_campaign_returns_404() {
    let app = common::spawn_app().await;

    let resp = reqwest::Client::new()
        .patch(format!("{}/campaign/nonexistent-id", app.base_url))
        .header("authorization", app.auth_header())
        .json(&wizard_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn patch_rejects_malformed_body_with_4xx() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .patch(format!("{}/campaign/test-id", app.base_url))
        .header("authorization", app.auth_header())
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

#[tokio::test]
async fn patch_without_auth_returns_401() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    let resp = reqwest::Client::new()
        .patch(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .json(&wizard_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn patch_without_wizard_complete_updates_name_only() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    Mock::given(method("PATCH"))
        .and(path(format!(
            "/internal/platform/campaign/{}",
            campaign_id.0
        )))
        .respond_with(ResponseTemplate::new(200))
        .mount(&app.platform)
        .await;

    let resp = reqwest::Client::new()
        .patch(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "name": "Renamed Campaign" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: CampaignMetadataResponse = resp.json().await.unwrap();
    assert_eq!(body.name, "Renamed Campaign");
    assert!(
        body.wizard_completed_at.is_none(),
        "wizard should not be completed"
    );
}
