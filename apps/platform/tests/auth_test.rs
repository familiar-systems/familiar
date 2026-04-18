mod common;

use familiar_systems_platform::entities::users;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use wiremock::{matchers::{method, path}, Mock, ResponseTemplate};

#[tokio::test]
async fn no_token_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::get(format!("{}/me", app.base_url)).await.unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn valid_token_returns_user_row_and_persists_it() {
    let app = common::spawn_app().await;
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": "test-sub",
                "email": {"address": "t@ex.com", "is_primary": true, "is_verified": true},
                "expiration": "2099-01-01T00:00:00Z",
                "session_id": "s"
            }
        })))
        .expect(1)
        .mount(&app.hanko)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/me", app.base_url))
        .header("authorization", "Bearer fake")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["hanko_sub"], "test-sub");

    let count = users::Entity::find()
        .filter(users::Column::HankoSub.eq("test-sub"))
        .all(&app.db)
        .await
        .unwrap()
        .len();
    assert_eq!(count, 1);
}

#[tokio::test]
async fn invalid_token_returns_401() {
    let app = common::spawn_app().await;
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&app.hanko)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/me", app.base_url))
        .header("authorization", "Bearer anything")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}

#[tokio::test]
async fn is_valid_false_returns_401() {
    let app = common::spawn_app().await;
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"is_valid": false})),
        )
        .expect(1)
        .mount(&app.hanko)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/me", app.base_url))
        .header("authorization", "Bearer anything")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
