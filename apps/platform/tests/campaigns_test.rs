mod common;

use familiar_systems_platform::entities::{campaigns, create_attempts};
use sea_orm::EntityTrait;
use serde_json::json;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{header, method, path, path_regex},
};

const SUB: &str = "0195b4a0-0000-7000-8000-000000000010";
const TOKEN: &str = "idem-token-001";

/// Mounts `POST /internal/campaign` and `PUT /internal/campaign/{id}/lease`
/// handlers on the campaign mock that respond 200.
async fn mock_campaign_create_and_lease_ok(app: &common::TestApp) {
    Mock::given(method("POST"))
        .and(path("/internal/campaign"))
        .and(header(
            "authorization",
            format!("Bearer {}", app.bearer).as_str(),
        ))
        .respond_with(ResponseTemplate::new(200))
        .mount(&app.campaign)
        .await;

    Mock::given(method("PUT"))
        .and(path_regex(r"/internal/campaign/.+/lease"))
        .and(header(
            "authorization",
            format!("Bearer {}", app.bearer).as_str(),
        ))
        .respond_with(ResponseTemplate::new(200))
        .mount(&app.campaign)
        .await;
}

#[tokio::test]
async fn create_campaign_mints_id_and_persists_routing_row() {
    let app = common::spawn_app().await;
    common::mock_hanko_user(&app, SUB, "create@ex.com").await;
    mock_campaign_create_and_lease_ok(&app).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/campaigns", app.base_url))
        .header("authorization", "Bearer test-token")
        .json(&json!({ "idempotency_token": TOKEN }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let campaign_id = body["campaign_id"]
        .as_str()
        .expect("campaign_id present")
        .to_string();
    assert!(
        !campaign_id.is_empty(),
        "campaign_id should be a non-empty nanoid"
    );

    // Routing row exists with NULL mirror columns.
    let row = campaigns::Entity::find_by_id(campaign_id.clone())
        .one(&app.db)
        .await
        .unwrap()
        .expect("campaign row should exist after create");
    assert!(row.name.is_none());
    assert!(row.tagline.is_none());
    assert!(row.game_system.is_none());
    assert!(row.content_locale.is_none());
    assert!(row.last_init_error.is_none());
    assert!(row.wizard_completed_at.is_none());

    // Idempotency row exists and points to the same campaign.
    let attempt = create_attempts::Entity::find_by_id(TOKEN.to_string())
        .one(&app.db)
        .await
        .unwrap()
        .expect("create_attempts row should exist");
    assert_eq!(attempt.campaign_id, campaign_id);
}

#[tokio::test]
async fn retry_with_same_token_returns_same_campaign_id() {
    let app = common::spawn_app().await;
    common::mock_hanko_user(&app, SUB, "retry@ex.com").await;
    mock_campaign_create_and_lease_ok(&app).await;

    let client = reqwest::Client::new();
    let body = json!({ "idempotency_token": TOKEN });

    let r1 = client
        .post(format!("{}/campaigns", app.base_url))
        .header("authorization", "Bearer test-token")
        .json(&body)
        .send()
        .await
        .unwrap();
    let id1: String = r1.json::<serde_json::Value>().await.unwrap()["campaign_id"]
        .as_str()
        .unwrap()
        .to_string();

    let r2 = client
        .post(format!("{}/campaigns", app.base_url))
        .header("authorization", "Bearer test-token")
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(r2.status().as_u16(), 200);
    let id2: String = r2.json::<serde_json::Value>().await.unwrap()["campaign_id"]
        .as_str()
        .unwrap()
        .to_string();

    assert_eq!(id1, id2, "retry must return the same campaign_id");

    // Exactly one routing row.
    let all = campaigns::Entity::find().all(&app.db).await.unwrap();
    assert_eq!(all.len(), 1);
}

#[tokio::test]
async fn shard_failure_returns_5xx_and_writes_no_routing_row() {
    let app = common::spawn_app().await;
    common::mock_hanko_user(&app, SUB, "fail@ex.com").await;
    Mock::given(method("POST"))
        .and(path("/internal/campaign"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&app.campaign)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/campaigns", app.base_url))
        .header("authorization", "Bearer test-token")
        .json(&json!({ "idempotency_token": TOKEN }))
        .send()
        .await
        .unwrap();
    assert!(
        resp.status().is_server_error(),
        "expected 5xx when shard create fails, got {}",
        resp.status()
    );
    // Routing row was never written.
    let all = campaigns::Entity::find().all(&app.db).await.unwrap();
    assert_eq!(all.len(), 0, "no routing row on shard failure");
}

#[tokio::test]
async fn no_token_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/campaigns", app.base_url))
        .json(&json!({ "idempotency_token": TOKEN }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn list_campaigns_returns_owners_rows_only() {
    let app = common::spawn_app().await;
    common::mock_hanko_user(&app, SUB, "list@ex.com").await;
    mock_campaign_create_and_lease_ok(&app).await;

    let client = reqwest::Client::new();
    // Create two campaigns under the authenticated user.
    for token in ["tok-a", "tok-b"] {
        let r = client
            .post(format!("{}/campaigns", app.base_url))
            .header("authorization", "Bearer test-token")
            .json(&json!({ "idempotency_token": token }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status().as_u16(), 200);
    }

    // GET /campaigns returns both, ordered most-recent-first.
    let resp = client
        .get(format!("{}/campaigns", app.base_url))
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    let arr = body.as_array().expect("response is array");
    assert_eq!(arr.len(), 2);
    // Most recent first: the second create's campaign_id should appear at index 0.
    assert!(arr[0]["created_at"].as_str().unwrap() >= arr[1]["created_at"].as_str().unwrap());
    // Mirrored fields are null on a freshly created campaign.
    assert!(arr[0]["name"].is_null());
    assert!(arr[0]["last_init_error"].is_null());
}

#[tokio::test]
async fn init_failed_writes_last_init_error_with_correct_bearer() {
    let app = common::spawn_app().await;
    common::mock_hanko_user(&app, SUB, "ifail@ex.com").await;
    mock_campaign_create_and_lease_ok(&app).await;

    // Create a campaign so we have an id to target.
    let client = reqwest::Client::new();
    let create = client
        .post(format!("{}/campaigns", app.base_url))
        .header("authorization", "Bearer test-token")
        .json(&json!({ "idempotency_token": TOKEN }))
        .send()
        .await
        .unwrap();
    let id: String = create.json::<serde_json::Value>().await.unwrap()["campaign_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Hit the platform's internal callback as the campaign tier would.
    let resp = client
        .post(format!(
            "{}/internal/platform/campaign/{}/init-failed",
            app.base_url, id
        ))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&json!({ "reason": "deliberate_thin_slice_failure" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let row = campaigns::Entity::find_by_id(id.clone())
        .one(&app.db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.last_init_error.as_deref(),
        Some("deliberate_thin_slice_failure")
    );
}

#[tokio::test]
async fn init_failed_without_bearer_returns_401() {
    let app = common::spawn_app().await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/internal/platform/campaign/abc/init-failed",
            app.base_url
        ))
        .json(&json!({ "reason": "no auth" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn init_failed_with_wrong_bearer_returns_401() {
    let app = common::spawn_app().await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/internal/platform/campaign/abc/init-failed",
            app.base_url
        ))
        .header("authorization", "Bearer wrong-token")
        .json(&json!({ "reason": "wrong auth" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
