mod common;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::registry::CreateCampaign;
use familiar_systems_campaign_shared::id::ThingId;
use serde_json::{Value, json};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

async fn create_campaign(app: &common::TestApp, campaign_id: &CampaignId) {
    let _: kameo::actor::ActorRef<
        familiar_systems_campaign::actors::supervisor::CampaignSupervisor,
    > = app
        .registry
        .ask(CreateCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: common::test_user_id(),
        })
        .await
        .expect("create campaign");
}

/// Mount the platform membership endpoint for this campaign+test-user, with
/// the given functional role (`"gm"` or `"player"`).
async fn mount_membership(app: &common::TestApp, campaign_id: &CampaignId, role: &str) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/internal/platform/campaign/{}/membership/{}",
            campaign_id.0,
            common::test_user_id().0
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "role": role })))
        .mount(&app.platform)
        .await;
}

#[tokio::test]
async fn gm_creates_thing_and_nests_under_parent() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let client = reqwest::Client::new();
    let url = format!("{}/campaign/{}/things", app.base_url, campaign_id.0);

    // Create a root-level Thing.
    let resp = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "name": "Korgath" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["name"], "Korgath");
    assert_eq!(body["status"], "gmOnly", "defaults to gm_only");
    assert!(body["prototype_id"].is_null());
    let parent_id = body["id"].as_str().expect("id is a string").to_string();

    // Create a child nested under the first Thing.
    let resp = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "name": "Korgath's Lair", "parent": parent_id }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        201,
        "nesting under a real parent succeeds"
    );
}

#[tokio::test]
async fn create_thing_with_unknown_parent_returns_422() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/{}/things",
            app.base_url, campaign_id.0
        ))
        .header("authorization", app.auth_header())
        .json(&json!({ "name": "Orphan", "parent": ThingId::generate().to_string() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 422);
}

#[tokio::test]
async fn player_cannot_create_thing() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "player").await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/{}/things",
            app.base_url, campaign_id.0
        ))
        .header("authorization", app.auth_header())
        .json(&json!({ "name": "Forbidden" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn create_thing_unknown_campaign_returns_404() {
    let app = common::spawn_app().await;

    // Not checked out on this shard -> 404 before the membership check.
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/nonexistent-id/things", app.base_url))
        .header("authorization", app.auth_header())
        .json(&json!({ "name": "Ghost" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn create_thing_from_template_returns_501() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    // `from_template_id` is refused before any other work, so no membership
    // mock is needed.
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/things", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "name": "From Template", "from_template_id": ThingId::generate().to_string() }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 501);
}

#[tokio::test]
async fn create_thing_without_auth_returns_401() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/{}/things",
            app.base_url, campaign_id.0
        ))
        .json(&json!({ "name": "Anon" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
