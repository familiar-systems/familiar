mod common;

#[tokio::test]
async fn spawn_app_serves_health() {
    let app = common::spawn_app().await;
    let resp = reqwest::get(format!("{}/health", app.base_url)).await.unwrap();
    assert_eq!(resp.status().as_u16(), 200);
}
