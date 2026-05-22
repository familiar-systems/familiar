mod common;

use familiar_systems_campaign::actors::registry::BeginDrain;
use serde_json::json;
use tokio::sync::oneshot;

const CAMPAIGN_ID: &str = "test-campaign-id-1";
const USER_ID: &str = "0195b4a0-0000-7000-8000-000000000099";

fn create_payload() -> serde_json::Value {
    json!({
        "campaign_id": CAMPAIGN_ID,
        "owner_user_id": USER_ID
    })
}

#[tokio::test]
async fn create_campaign_with_correct_bearer_returns_200_and_creates_db_file() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign", app.base_url))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&create_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let db_path = app.data_dir.path().join(format!("{CAMPAIGN_ID}.db"));
    assert!(
        db_path.exists(),
        "expected campaign DB at {db_path:?} to exist after create"
    );
}

#[tokio::test]
async fn repeat_create_is_idempotent() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();
    let post = || async {
        client
            .post(format!("{}/internal/campaign", app.base_url))
            .header("authorization", format!("Bearer {}", app.bearer))
            .json(&create_payload())
            .send()
            .await
            .unwrap()
    };
    assert_eq!(post().await.status().as_u16(), 200);
    assert_eq!(post().await.status().as_u16(), 200);
    let db_path = app.data_dir.path().join(format!("{CAMPAIGN_ID}.db"));
    assert!(db_path.exists());
}

#[tokio::test]
async fn create_without_bearer_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign", app.base_url))
        .json(&create_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn create_during_drain_returns_503() {
    let app = common::spawn_app().await;
    let (tx, rx) = oneshot::channel();
    app.registry
        .ask(BeginDrain { completion: tx })
        .await
        .expect("BeginDrain");
    rx.await.expect("drain completion");

    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign", app.base_url))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&create_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 503);
}

#[tokio::test]
async fn create_with_wrong_bearer_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign", app.base_url))
        .header("authorization", "Bearer wrong")
        .json(&create_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn acquire_lease_returns_200_for_existing_campaign() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();

    // Create the campaign first.
    let resp = client
        .post(format!("{}/internal/campaign", app.base_url))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&create_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    // Acquire lease (idempotent, already checked out).
    let resp = client
        .put(format!(
            "{}/internal/campaign/{}/lease",
            app.base_url, CAMPAIGN_ID
        ))
        .header("authorization", format!("Bearer {}", app.bearer))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

#[tokio::test]
async fn acquire_lease_without_bearer_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .put(format!(
            "{}/internal/campaign/{}/lease",
            app.base_url, CAMPAIGN_ID
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn release_lease_returns_200_for_loaded_campaign() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();

    client
        .post(format!("{}/internal/campaign", app.base_url))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&create_payload())
        .send()
        .await
        .unwrap();

    let resp = client
        .delete(format!(
            "{}/internal/campaign/{}/lease",
            app.base_url, CAMPAIGN_ID
        ))
        .header("authorization", format!("Bearer {}", app.bearer))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

#[tokio::test]
async fn release_lease_returns_200_for_unknown_campaign() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/internal/campaign/{}/lease",
            app.base_url, "never-created"
        ))
        .header("authorization", format!("Bearer {}", app.bearer))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

#[tokio::test]
async fn release_lease_without_bearer_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .delete(format!(
            "{}/internal/campaign/{}/lease",
            app.base_url, CAMPAIGN_ID
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
