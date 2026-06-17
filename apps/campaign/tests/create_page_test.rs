mod common;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::registry::CreateCampaign;
use familiar_systems_campaign_shared::id::PageId;
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
async fn gm_creates_page_and_nests_under_parent() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let client = reqwest::Client::new();
    let url = format!("{}/campaign/{}/pages", app.base_url, campaign_id.0);

    // Create a root-level entity. The request and response are kind-tagged:
    // `{ kind, content: { ... } }`.
    let resp = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Korgath" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["kind"], "entity");
    assert_eq!(body["content"]["name"], "Korgath");
    assert_eq!(body["content"]["status"], "gmOnly", "defaults to gm_only");
    assert!(body["content"]["template_id"].is_null());
    let parent_id = body["content"]["id"]
        .as_str()
        .expect("id is a string")
        .to_string();

    // Create a child nested under the first entity.
    let resp = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Korgath's Lair", "parent": parent_id } }))
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
async fn gm_creates_template() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "template", "content": { "name": "NPC" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["kind"], "template");
    assert_eq!(body["content"]["name"], "NPC");
    // A template carries no `template_id` lineage of its own; the response
    // variant has no such field.
    assert!(body["content"]["template_id"].is_null());
    assert!(body["content"]["id"].as_str().is_some());
}

#[tokio::test]
async fn gm_creates_session_via_pages() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    // An unnamed session: all body fields are optional.
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "session", "content": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["kind"], "session");
    // The session response carries the temporal record the page form does not.
    assert_eq!(
        body["content"]["ordinal"], 1,
        "first session in the campaign"
    );
    // An unnamed session stores an empty name: it is identified by its ordinal,
    // and the client composes "Session {ordinal}" for display.
    assert_eq!(body["content"]["name"], "");
    assert!(body["content"]["page_id"].as_str().is_some());
    assert!(body["content"]["session_id"].as_str().is_some());
}

#[tokio::test]
async fn create_page_with_unknown_parent_returns_422() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Orphan", "parent": PageId::generate().to_string() } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 422);
}

#[tokio::test]
async fn player_cannot_create_page() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "player").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Forbidden" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn create_page_unknown_campaign_returns_404() {
    let app = common::spawn_app().await;

    // Not checked out on this shard -> 404 (after the body parses but before the
    // membership check).
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/nonexistent-id/pages", app.base_url))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Ghost" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

#[tokio::test]
async fn create_page_from_template_returns_501() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    // `from_template_id` (entity clone) is refused before any other work, so no
    // membership mock is needed.
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({
            "kind": "entity",
            "content": { "name": "From Template", "from_template_id": PageId::generate().to_string() }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 501);
}

#[tokio::test]
async fn create_page_without_auth_returns_401() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .json(&json!({ "kind": "entity", "content": { "name": "Anon" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
