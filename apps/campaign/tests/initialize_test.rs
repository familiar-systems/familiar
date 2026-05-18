mod common;

use serde_json::json;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{body_partial_json, header, method, path},
};

const CAMPAIGN_ID: &str = "test-campaign-1";

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

#[tokio::test]
async fn initialize_fires_platform_callback_and_returns_500() {
    let app = common::spawn_app().await;

    // The platform receives the init-failed callback with the bearer attached.
    Mock::given(method("POST"))
        .and(path(format!(
            "/internal/platform/campaigns/{CAMPAIGN_ID}/init-failed"
        )))
        .and(header(
            "authorization",
            format!("Bearer {}", app.bearer).as_str(),
        ))
        .and(body_partial_json(
            json!({"reason": "deliberate_thin_slice_failure"}),
        ))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&app.platform)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/{}/initialize",
            app.base_url, CAMPAIGN_ID
        ))
        .json(&valid_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 500);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["campaign_id"], CAMPAIGN_ID);
    assert!(
        body["error"].as_str().unwrap().contains("not yet wired up"),
        "expected deliberate-failure copy, got {}",
        body["error"]
    );
}

#[tokio::test]
async fn initialize_still_returns_500_when_platform_callback_itself_errors() {
    // If the platform's init-failed handler is down, the campaign tier still
    // returns the deliberate 500 to the SPA so the FE-visible failure stays
    // stable. The dropped callback gets logged at warn (not asserted here).
    let app = common::spawn_app().await;

    Mock::given(method("POST"))
        .and(path(format!(
            "/internal/platform/campaigns/{CAMPAIGN_ID}/init-failed"
        )))
        .respond_with(ResponseTemplate::new(500))
        .mount(&app.platform)
        .await;

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/{}/initialize",
            app.base_url, CAMPAIGN_ID
        ))
        .json(&valid_payload())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 500);
}

#[tokio::test]
async fn initialize_rejects_malformed_body_with_4xx() {
    // axum's Json extractor 422s on parse errors; the test pins behaviour so
    // the FE knows what to expect when its zod schema generates the wrong
    // shape.
    let app = common::spawn_app().await;
    let resp = reqwest::Client::new()
        .post(format!(
            "{}/campaign/{}/initialize",
            app.base_url, CAMPAIGN_ID
        ))
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
