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
        .header("authorization", "Bearer test-token")
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
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
    let count = users::Entity::find().all(&app.db).await.unwrap().len();
    assert_eq!(count, 0, "rejected request must not insert a user row");
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
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
    let count = users::Entity::find().all(&app.db).await.unwrap().len();
    assert_eq!(count, 0, "rejected request must not insert a user row");
}

#[tokio::test]
async fn upsert_is_idempotent() {
    let app = common::spawn_app().await;
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": "idempotent-sub",
                "email": {"address": "i@ex.com", "is_primary": true, "is_verified": true},
                "expiration": "2099-01-01T00:00:00Z",
                "session_id": "s"
            }
        })))
        .expect(2)
        .mount(&app.hanko)
        .await;

    let client = reqwest::Client::new();

    // First call: insert.
    let resp1 = client
        .get(format!("{}/me", app.base_url))
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status().as_u16(), 200);
    let row1 = users::Entity::find()
        .filter(users::Column::HankoSub.eq("idempotent-sub"))
        .one(&app.db)
        .await
        .unwrap()
        .expect("first call should insert the row");
    let updated_at_1 = row1.updated_at;

    // Sleep so updated_at advances on conflict.
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;

    // Second call: conflict path updates email + updated_at.
    let resp2 = client
        .get(format!("{}/me", app.base_url))
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp2.status().as_u16(), 200);

    let rows = users::Entity::find()
        .filter(users::Column::HankoSub.eq("idempotent-sub"))
        .all(&app.db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1, "upsert must produce exactly one row across two calls");
    assert!(
        rows[0].updated_at > updated_at_1,
        "updated_at must advance on conflict; was {updated_at_1:?}, now {:?}",
        rows[0].updated_at,
    );
}
