mod common;

use familiar_systems_campaign::actors::registry::BeginDrain;
use serde_json::json;
use tokio::sync::oneshot;

const CAMPAIGN_ID: &str = "test-campaign-id-1";
const USER_ID: &str = "0195b4a0-0000-7000-8000-000000000099";

fn payload() -> serde_json::Value {
    json!({
        "campaign_id": CAMPAIGN_ID,
        "owner_user_id": USER_ID
    })
}

#[tokio::test]
async fn init_with_correct_bearer_returns_200_and_creates_db_file() {
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign/init", app.base_url))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);

    let db_path = app.data_dir.path().join(format!("{CAMPAIGN_ID}.db"));
    assert!(
        db_path.exists(),
        "expected campaign DB at {db_path:?} to exist after init"
    );
}

#[tokio::test]
async fn repeat_init_is_idempotent() {
    // The platform retries init on transient failures; calling twice with
    // the same campaign_id must return 200 both times without spawning a
    // second supervisor or overwriting the existing DB. The registry's
    // mailbox serializes ensure calls, so concurrent or sequential repeats
    // converge on the single live supervisor.
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();
    let post = || async {
        client
            .post(format!("{}/internal/campaign/init", app.base_url))
            .header("authorization", format!("Bearer {}", app.bearer))
            .json(&payload())
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
async fn init_during_drain_returns_503() {
    // Once the registry has entered its drain phase, the create-side
    // hook must return 503 so the platform retries against whichever
    // shard takes the campaign over. A 500 here would be misread as a
    // terminal init failure rather than "wrong shard."
    let app = common::spawn_app().await;
    let (tx, rx) = oneshot::channel();
    app.registry
        .ask(BeginDrain { completion: tx })
        .await
        .expect("BeginDrain");
    rx.await.expect("drain completion");

    let resp = reqwest::Client::new()
        .post(format!("{}/internal/campaign/init", app.base_url))
        .header("authorization", format!("Bearer {}", app.bearer))
        .json(&payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 503);
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
