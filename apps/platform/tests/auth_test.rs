mod common;

#[tokio::test]
async fn no_token_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::get(format!("{}/me", app.base_url)).await.unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
