mod common;

use serde_json::json;

const CAMPAIGN_ID: &str = "test-campaign-id-1";
const USER_ID: &str = "0195b4a0-0000-7000-8000-000000000099";

fn payload() -> serde_json::Value {
    json!({
        "campaign_id": CAMPAIGN_ID,
        "owner_user_id": USER_ID
    })
}

#[tokio::test]
async fn init_with_correct_bearer_returns_200() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign/init", app.base_url))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}

#[tokio::test]
async fn init_without_bearer_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign/init", app.base_url))
        .json(&payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn init_with_wrong_bearer_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign/init", app.base_url))
        .header("authorization", "Bearer wrong")
        .json(&payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
