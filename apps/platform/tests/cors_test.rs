mod common;

#[tokio::test]
async fn preflight_allows_listed_origin() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::OPTIONS, format!("{}/me", app.base_url))
        .header("origin", "http://localhost:5173")
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "authorization")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "preflight should succeed");
    let allow_origin = resp
        .headers()
        .get("access-control-allow-origin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    assert_eq!(
        allow_origin.as_deref(),
        Some("http://localhost:5173"),
        "expected echoed origin",
    );
}

#[tokio::test]
async fn preflight_rejects_other_origin() {
    let app = common::spawn_app().await;
    let client = reqwest::Client::new();
    let resp = client
        .request(reqwest::Method::OPTIONS, format!("{}/me", app.base_url))
        .header("origin", "http://evil.example.com")
        .header("access-control-request-method", "GET")
        .header("access-control-request-headers", "authorization")
        .send()
        .await
        .unwrap();
    // When origin is rejected, tower-http's CorsLayer does not emit Access-Control-Allow-Origin.
    // The HTTP status is still 200 (preflight is processed), but no ACA-Origin header is returned.
    assert!(
        resp.headers().get("access-control-allow-origin").is_none(),
        "rejected origin must not get ACA-Origin header",
    );
}
