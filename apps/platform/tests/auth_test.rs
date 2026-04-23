mod common;

use familiar_systems_platform::entities::users;
use sea_orm::{ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

// Fixed UUIDs kept as constants so the mock body and the DB assertions agree
// on one value. v7 shape so they sort and look like real Hanko-minted ids.
const SUB_A: &str = "0195b4a0-0000-7000-8000-000000000001";
const SUB_B: &str = "0195b4a0-0000-7000-8000-000000000002";
const SUB_IDEMPOTENT: &str = "0195b4a0-0000-7000-8000-000000000003";

#[tokio::test]
async fn no_token_returns_401() {
    let app = common::spawn_app().await;
    let resp = reqwest::get(format!("{}/me", app.base_url)).await.unwrap();
    assert_eq!(resp.status().as_u16(), 401);
    let body: serde_json::Value = resp.json().await.unwrap();
    // Unauthorized(m) echoes its caller-facing message; no PII, no DB detail.
    assert_eq!(body["error"], "missing authorization header");
}

#[tokio::test]
async fn valid_token_returns_user_row_and_persists_it() {
    let app = common::spawn_app().await;
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": SUB_A,
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
    assert_eq!(body["id"], SUB_A);
    assert_eq!(body["email"], "t@ex.com");

    let sub_a_uuid = uuid::Uuid::parse_str(SUB_A).unwrap();
    let count = users::Entity::find()
        .filter(users::Column::Id.eq(sub_a_uuid))
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
    // Lock the hardening guarantee: Auth errors must return a generic body,
    // never echo the underlying reqwest/Hanko detail (which can include the
    // tenant URL) back to the client.
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "unauthorized");
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
                "subject": SUB_IDEMPOTENT,
                "email": {"address": "i@ex.com", "is_primary": true, "is_verified": true},
                "expiration": "2099-01-01T00:00:00Z",
                "session_id": "s"
            }
        })))
        .expect(2)
        .mount(&app.hanko)
        .await;

    let client = reqwest::Client::new();
    let sub_uuid = uuid::Uuid::parse_str(SUB_IDEMPOTENT).unwrap();

    // First call: insert.
    let resp1 = client
        .get(format!("{}/me", app.base_url))
        .header("authorization", "Bearer test-token")
        .send()
        .await
        .unwrap();
    assert_eq!(resp1.status().as_u16(), 200);
    let row1 = users::Entity::find()
        .filter(users::Column::Id.eq(sub_uuid))
        .one(&app.db)
        .await
        .unwrap()
        .expect("first call should insert the row");
    let updated_at_1 = row1.updated_at;

    // 10ms is well above SQLite TEXT timestamp precision (microseconds via ISO 8601),
    // so updated_at is guaranteed to advance on the conflict path.
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
        .filter(users::Column::Id.eq(sub_uuid))
        .all(&app.db)
        .await
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "upsert must produce exactly one row across two calls"
    );
    assert!(
        rows[0].updated_at > updated_at_1,
        "updated_at must advance on conflict; was {updated_at_1:?}, now {:?}",
        rows[0].updated_at,
    );
}

#[tokio::test]
async fn email_collision_returns_409() {
    // A pre-existing local row holds an email, and a different Hanko
    // subject now logs in with that same email. Hanko wouldn't permit this
    // in real time (emails are unique per user in the tenant) but our
    // local mirror can lag behind Hanko's current state. Expected: clean
    // 409 with a generic body, no partial write.
    let app = common::spawn_app().await;

    // Seed the pre-existing row directly (bypassing Hanko validation).
    let existing_id = uuid::Uuid::parse_str(SUB_B).unwrap();
    let now = chrono::Utc::now();
    users::Entity::insert(users::ActiveModel {
        id: Set(existing_id),
        email: Set("shared@ex.com".into()),
        created_at: Set(now),
        updated_at: Set(now),
    })
    .exec(&app.db)
    .await
    .unwrap();

    // Now validate a DIFFERENT subject claiming the same email.
    Mock::given(method("POST"))
        .and(path("/sessions/validate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "is_valid": true,
            "claims": {
                "subject": SUB_A,
                "email": {"address": "shared@ex.com", "is_primary": true, "is_verified": true},
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
    assert_eq!(
        resp.status().as_u16(),
        409,
        "email collision must surface as 409, not 500"
    );
    let body: serde_json::Value = resp.json().await.unwrap();
    // Generic message; no sea-orm detail, no email echo in the response.
    assert_eq!(
        body["error"],
        "email already registered to another account; contact support"
    );

    let all = users::Entity::find().all(&app.db).await.unwrap();
    assert_eq!(all.len(), 1, "collision path must not create a second row");
    assert_eq!(all[0].id, existing_id);
}
