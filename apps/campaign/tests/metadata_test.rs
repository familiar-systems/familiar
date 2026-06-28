mod common;

use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign::actors::registry::{CreateCampaign, resolve};
use familiar_systems_campaign_shared::onboarding::metadata::CampaignMetadataResponse;
use serde_json::json;
use std::time::Duration;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

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

fn wizard_payload() -> serde_json::Value {
    json!({
        "game_system": "dnd-5e",
        "content_locale": "en",
        "name": "Embergrove Saga",
        "tagline": "An autumn court, a debt come due.",
        "template_slugs": ["common/npc", "common/player"],
        "audio": "opt-out",
        "evals_enabled": false,
        "wizard_complete": true
    })
}

#[tokio::test]
async fn get_campaign_returns_metadata_after_patch() {
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
    let patch_resp = client
        .patch(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&wizard_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(patch_resp.status().as_u16(), 200);

    let get_resp = client
        .get(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(get_resp.status().as_u16(), 200);

    let body: CampaignMetadataResponse = get_resp.json().await.unwrap();
    assert_eq!(body.campaign_id, campaign_id.0.to_string());
    assert_eq!(body.name, "Embergrove Saga");
    assert_eq!(
        body.tagline.as_deref(),
        Some("An autumn court, a debt come due.")
    );
    assert_eq!(body.game_system.as_deref(), Some("dnd-5e"));
    assert_eq!(body.content_locale.as_deref(), Some("en"));
    assert!(body.wizard_completed_at.is_some());
}

#[tokio::test]
async fn get_campaign_returns_draft_before_patch() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let body: CampaignMetadataResponse = resp.json().await.unwrap();
    assert_eq!(body.campaign_id, campaign_id.0.to_string());
    assert!(body.wizard_completed_at.is_none());
}

#[tokio::test]
async fn get_unknown_campaign_returns_404() {
    let app = common::spawn_app().await;

    let resp = reqwest::Client::new()
        .get(format!("{}/campaign/nonexistent-id", app.base_url))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn get_campaign_without_auth_returns_401() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    let resp = reqwest::Client::new()
        .get(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn get_campaign_by_different_user_returns_403() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();

    let state = app
        .registry
        .ask(CreateCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: UserId::generate(),
        })
        .await
        .expect("create campaign");
    resolve(Some(state), Duration::from_secs(30))
        .await
        .expect("campaign ready");

    let resp = reqwest::Client::new()
        .get(format!("{}/campaign/{}", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}
