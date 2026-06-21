//! Relationship REST surface: create / patch / delete / read over real HTTP, against
//! the full `TestApp` stack (router + supervisor + RelationshipGraph + SQLite, with
//! wiremock standins for platform membership and Hanko). Asserts the verb-oriented
//! surface the widget calls, the atomic supersede-via-`supersedes`, and the GM gate.

mod common;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::registry::CreateCampaign;
use familiar_systems_campaign_shared::id::{PageId, RelationshipId, SessionId};
use reqwest::Client;
use serde_json::{Value, json};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

// ---------------------------------------------------------------------------
// Harness helpers
// ---------------------------------------------------------------------------

async fn create_campaign(app: &common::TestApp, campaign_id: &CampaignId) {
    let _: kameo::actor::ActorRef<
        familiar_systems_campaign::actors::supervisor::CampaignSupervisor,
    > = app
        .registry
        .ask(CreateCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: common::test_user_id(),
        })
        .await
        .expect("create campaign");
}

async fn mount_membership(app: &common::TestApp, campaign_id: &CampaignId, role: &str) {
    Mock::given(method("GET"))
        .and(path(format!(
            "/internal/platform/campaign/{}/membership/{}",
            campaign_id.0,
            common::test_user_id().0
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "role": role })))
        .mount(&app.platform)
        .await;
}

/// A GM-authed campaign with membership mounted; the common starting point.
async fn gm_campaign(app: &common::TestApp) -> CampaignId {
    let campaign_id = CampaignId::generate();
    create_campaign(app, &campaign_id).await;
    mount_membership(app, &campaign_id, "gm").await;
    campaign_id
}

async fn create_page(app: &common::TestApp, cid: &CampaignId, kind: &str, name: &str) -> Value {
    let resp = Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, cid.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": kind, "content": { "name": name } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201, "page create");
    resp.json::<Value>().await.unwrap()["content"].clone()
}

async fn create_entity(app: &common::TestApp, cid: &CampaignId, name: &str) -> String {
    create_page(app, cid, "entity", name).await["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// Create a session, returning its durable `session_id` and curated ordinal.
async fn create_session(app: &common::TestApp, cid: &CampaignId, name: &str) -> (String, i64) {
    let content = create_page(app, cid, "session", name).await;
    (
        content["session_id"].as_str().unwrap().to_string(),
        content["ordinal"].as_i64().unwrap(),
    )
}

async fn post_relationship(
    app: &common::TestApp,
    cid: &CampaignId,
    body: Value,
) -> reqwest::Response {
    Client::new()
        .post(format!("{}/campaign/{}/relationships", app.base_url, cid.0))
        .header("authorization", app.auth_header())
        .json(&body)
        .send()
        .await
        .unwrap()
}

async fn get_relationships(app: &common::TestApp, cid: &CampaignId, page_id: &str) -> Vec<Value> {
    let resp = Client::new()
        .get(format!(
            "{}/campaign/{}/pages/{}/relationships",
            app.base_url, cid.0, page_id
        ))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200, "list relationships");
    resp.json::<Vec<Value>>().await.unwrap()
}

/// Create a Players-visible, Prior-origin relationship and return its id.
async fn seed_relationship(
    app: &common::TestApp,
    cid: &CampaignId,
    subject: &str,
    other: &str,
    fwd: &str,
    rev: &str,
) -> String {
    let resp = post_relationship(
        app,
        cid,
        json!({
            "subject_page_id": subject,
            "other_page_id": other,
            "predicate_forward": fwd,
            "predicate_reverse": rev,
            "visibility": "players",
            "origin": { "kind": "prior" },
        }),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 201, "seed relationship");
    resp.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

// ---------------------------------------------------------------------------
// Create + read
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_orients_and_lists_from_both_pages() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let wren = create_entity(&app, &cid, "Wren Aldwater").await;
    let town = create_entity(&app, &cid, "Grimhollow").await;

    let resp = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": wren,
            "other_page_id": town,
            "predicate_forward": "is a resident of",
            "predicate_reverse": "is the home of",
            "visibility": "players",
            "origin": { "kind": "prior" },
        }),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 201);
    let view: Value = resp.json().await.unwrap();
    assert_eq!(view["predicate"], "is a resident of");
    assert_eq!(view["other"]["name"], "Grimhollow");
    assert_eq!(view["origin"]["kind"], "prior");
    assert!(view["invalidation"].is_null());

    let from_wren = get_relationships(&app, &cid, &wren).await;
    assert_eq!(from_wren.len(), 1);
    assert_eq!(from_wren[0]["predicate"], "is a resident of");

    // The far page reads the same edge reversed.
    let from_town = get_relationships(&app, &cid, &town).await;
    assert_eq!(from_town[0]["predicate"], "is the home of");
    assert_eq!(from_town[0]["other"]["name"], "Wren Aldwater");
}

#[tokio::test]
async fn duplicate_live_fact_is_409() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;

    seed_relationship(&app, &cid, &a, &b, "rules", "is ruled by").await;
    let dup = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": a, "other_page_id": b,
            "predicate_forward": "rules", "predicate_reverse": "is ruled by",
            "visibility": "players", "origin": { "kind": "prior" },
        }),
    )
    .await;
    assert_eq!(dup.status().as_u16(), 409);
}

#[tokio::test]
async fn self_edge_is_422() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;

    let resp = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": a, "other_page_id": a,
            "predicate_forward": "knows", "predicate_reverse": "knows",
            "visibility": "players", "origin": { "kind": "prior" },
        }),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 422);
}

// ---------------------------------------------------------------------------
// Supersede (atomic, via the `supersedes` pointer on create)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn supersede_ends_old_and_creates_new_atomically() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let (s1, ord1) = create_session(&app, &cid, "Session One").await;
    let (s2, ord2) = create_session(&app, &cid, "Session Two").await;

    let original = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": a, "other_page_id": b,
            "predicate_forward": "is captain of", "predicate_reverse": "is captained by",
            "visibility": "players", "origin": { "kind": "session", "content": s1 },
        }),
    )
    .await;
    let original_id = original.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();

    let replacement = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": a, "other_page_id": b,
            "predicate_forward": "is quartermaster of", "predicate_reverse": "is quartermastered by",
            "visibility": "players", "origin": { "kind": "session", "content": s2 },
            "supersedes": original_id,
        }),
    )
    .await;
    assert_eq!(replacement.status().as_u16(), 201);
    let new_view: Value = replacement.json().await.unwrap();
    assert_eq!(new_view["predicate"], "is quartermaster of");
    assert!(
        new_view["invalidation"].is_null(),
        "the reply is the live new row"
    );
    assert_eq!(new_view["origin"]["content"]["ordinal"], ord2);

    let rows = get_relationships(&app, &cid, &a).await;
    assert_eq!(rows.len(), 2, "old (superseded) + new (live)");
    let old = rows.iter().find(|r| r["id"] == original_id).unwrap();
    assert_eq!(
        old["predicate"], "is captain of",
        "old predicate is immutable"
    );
    assert_eq!(old["invalidation"]["kind"], "superseded");
    assert_eq!(
        old["invalidation"]["content"]["ended"]["content"]["ordinal"],
        ord2
    );
    let _ = ord1;
}

#[tokio::test]
async fn supersede_with_prior_origin_is_422() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let original = seed_relationship(&app, &cid, &a, &b, "is captain of", "is captained by").await;

    let resp = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": a, "other_page_id": b,
            "predicate_forward": "is admiral of", "predicate_reverse": "is commanded by",
            "visibility": "players", "origin": { "kind": "prior" },
            "supersedes": original,
        }),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 422, "cannot supersede as of prior");
}

// ---------------------------------------------------------------------------
// Patch (visibility + invalidation) and delete
// ---------------------------------------------------------------------------

async fn patch_relationship(
    app: &common::TestApp,
    cid: &CampaignId,
    rel_id: &str,
    body: Value,
) -> u16 {
    Client::new()
        .patch(format!(
            "{}/campaign/{}/relationships/{}",
            app.base_url, cid.0, rel_id
        ))
        .header("authorization", app.auth_header())
        .json(&body)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn patch_visibility() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "is suspicious of", "is distrusted by").await;

    assert_eq!(
        patch_relationship(&app, &cid, &rel, json!({ "visibility": "gm" })).await,
        204
    );
    let rows = get_relationships(&app, &cid, &a).await;
    assert_eq!(rows[0]["visibility"], "gm");
    assert!(
        rows[0]["invalidation"].is_null(),
        "visibility change does not invalidate"
    );
}

#[tokio::test]
async fn patch_end_marks_superseded() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let (s1, ord1) = create_session(&app, &cid, "Session One").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "is captain of", "is captained by").await;

    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &rel,
            json!({ "invalidation": { "reason": "superseded", "as_of": s1 } })
        )
        .await,
        204
    );
    let rows = get_relationships(&app, &cid, &a).await;
    assert_eq!(rows[0]["invalidation"]["kind"], "superseded");
    assert_eq!(
        rows[0]["invalidation"]["content"]["ended"]["content"]["ordinal"],
        ord1
    );
}

#[tokio::test]
async fn patch_end_without_as_of_is_422() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "is captain of", "is captained by").await;

    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &rel,
            json!({ "invalidation": { "reason": "superseded" } })
        )
        .await,
        422,
        "ending requires an as-of session"
    );
}

#[tokio::test]
async fn patch_retcon_marks_retconned() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "is brother to", "is brother to").await;

    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &rel,
            json!({ "invalidation": { "reason": "retconned" } })
        )
        .await,
        204
    );
    let rows = get_relationships(&app, &cid, &a).await;
    assert_eq!(rows[0]["invalidation"]["kind"], "retconned");
}

#[tokio::test]
async fn patch_empty_is_422() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "knows", "is known by").await;

    assert_eq!(patch_relationship(&app, &cid, &rel, json!({})).await, 422);
}

#[tokio::test]
async fn patch_unknown_relationship_is_404() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;

    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &RelationshipId::generate().to_string(),
            json!({ "visibility": "gm" })
        )
        .await,
        404
    );
}

#[tokio::test]
async fn delete_removes_relationship() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "owes a debt to", "holds marker on").await;

    let status = Client::new()
        .delete(format!(
            "{}/campaign/{}/relationships/{}",
            app.base_url, cid.0, rel
        ))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 204);
    assert!(get_relationships(&app, &cid, &a).await.is_empty());
}

// ---------------------------------------------------------------------------
// Invalidation guards (findings 1-3)
// ---------------------------------------------------------------------------

/// Create a relationship with a session origin, returning its id.
async fn seed_rel_at_session(
    app: &common::TestApp,
    cid: &CampaignId,
    subject: &str,
    other: &str,
    session_id: &str,
) -> String {
    let resp = post_relationship(
        app,
        cid,
        json!({
            "subject_page_id": subject, "other_page_id": other,
            "predicate_forward": "is captain of", "predicate_reverse": "is captained by",
            "visibility": "players", "origin": { "kind": "session", "content": session_id },
        }),
    )
    .await;
    assert_eq!(
        resp.status().as_u16(),
        201,
        "seed session-origin relationship"
    );
    resp.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn patch_end_on_ended_is_409() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let (s1, _) = create_session(&app, &cid, "Session One").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "is captain of", "is captained by").await;

    let end = json!({ "invalidation": { "reason": "superseded", "as_of": s1 } });
    assert_eq!(patch_relationship(&app, &cid, &rel, end.clone()).await, 204);
    // The one-way door: a second End cannot rewrite an already-ended row.
    assert_eq!(patch_relationship(&app, &cid, &rel, end).await, 409);
}

#[tokio::test]
async fn patch_retcon_on_superseded_is_409() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let (s1, _) = create_session(&app, &cid, "Session One").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "is captain of", "is captained by").await;

    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &rel,
            json!({ "invalidation": { "reason": "superseded", "as_of": s1 } })
        )
        .await,
        204
    );
    // A superseded row cannot be flipped to retconned (that would change its snapshot
    // visibility); reject it rather than silently reclassify.
    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &rel,
            json!({ "invalidation": { "reason": "retconned" } })
        )
        .await,
        409
    );
}

#[tokio::test]
async fn create_with_unknown_origin_session_is_404() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;

    let resp = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": a, "other_page_id": b,
            "predicate_forward": "is captain of", "predicate_reverse": "is captained by",
            "visibility": "players",
            "origin": { "kind": "session", "content": SessionId::generate().to_string() },
        }),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 404, "unknown origin session");
}

#[tokio::test]
async fn patch_end_with_unknown_as_of_is_404() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let rel = seed_relationship(&app, &cid, &a, &b, "is captain of", "is captained by").await;

    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &rel,
            json!({ "invalidation": { "reason": "superseded", "as_of": SessionId::generate().to_string() } })
        )
        .await,
        404,
        "unknown as-of session"
    );
}

#[tokio::test]
async fn end_before_origin_is_422() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let (s1, _) = create_session(&app, &cid, "Session One").await;
    let (s2, _) = create_session(&app, &cid, "Session Two").await;
    // Born at S2, then asked to end as of the earlier S1: a fact cannot end before it began.
    let rel = seed_rel_at_session(&app, &cid, &a, &b, &s2).await;

    assert_eq!(
        patch_relationship(
            &app,
            &cid,
            &rel,
            json!({ "invalidation": { "reason": "superseded", "as_of": s1 } })
        )
        .await,
        422
    );
}

#[tokio::test]
async fn supersede_before_origin_is_422() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let (s1, _) = create_session(&app, &cid, "Session One").await;
    let (s2, _) = create_session(&app, &cid, "Session Two").await;
    let original = seed_rel_at_session(&app, &cid, &a, &b, &s2).await;

    // Replacement dated S1, earlier than the S2 fact it supersedes.
    let resp = post_relationship(
        &app,
        &cid,
        json!({
            "subject_page_id": a, "other_page_id": b,
            "predicate_forward": "is admiral of", "predicate_reverse": "is commanded by",
            "visibility": "players", "origin": { "kind": "session", "content": s1 },
            "supersedes": original,
        }),
    )
    .await;
    assert_eq!(resp.status().as_u16(), 422);
}

// ---------------------------------------------------------------------------
// Auxiliary reads
// ---------------------------------------------------------------------------

#[tokio::test]
async fn predicates_endpoint_clusters_with_counts() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let a = create_entity(&app, &cid, "A").await;
    let b = create_entity(&app, &cid, "B").await;
    let c = create_entity(&app, &cid, "C").await;
    seed_relationship(&app, &cid, &a, &b, "is a resident of", "is the home of").await;
    seed_relationship(&app, &cid, &a, &c, "is a resident of", "is the home of").await;
    seed_relationship(&app, &cid, &b, &c, "is allied with", "is allied with").await;

    let resp = Client::new()
        .get(format!(
            "{}/campaign/{}/relationships/predicates",
            app.base_url, cid.0
        ))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let pairs: Vec<Value> = resp.json().await.unwrap();
    assert_eq!(pairs.len(), 2);
    let resident = pairs
        .iter()
        .find(|p| {
            [p["forward"].as_str(), p["reverse"].as_str()].contains(&Some("is a resident of"))
        })
        .expect("resident/home pair present");
    assert_eq!(resident["count"], 2);
}

#[tokio::test]
async fn sessions_endpoint_returns_current() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    let (s1, _) = create_session(&app, &cid, "Session One").await;
    create_session(&app, &cid, "Session Two").await;

    let resp = Client::new()
        .get(format!("{}/campaign/{}/sessions", app.base_url, cid.0))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["sessions"].as_array().unwrap().len(), 2);
    assert_eq!(body["current"]["ordinal"], 2, "current is the max ordinal");
    let _ = s1;
}

#[tokio::test]
async fn entities_search_excludes_templates() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;
    create_entity(&app, &cid, "Wren Aldwater").await;
    create_page(&app, &cid, "template", "NPC").await;

    let search = |q: &str| {
        let app = &app;
        let cid = &cid;
        let q = q.to_string();
        async move {
            Client::new()
                .get(format!("{}/campaign/{}/entities", app.base_url, cid.0))
                .query(&[("q", q.as_str())])
                .header("authorization", app.auth_header())
                .send()
                .await
                .unwrap()
                .json::<Vec<Value>>()
                .await
                .unwrap()
        }
    };

    let hits = search("Aldwater").await;
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["name"], "Wren Aldwater");

    // The template's name matches the query, but a template is not an entity.
    assert!(search("NPC").await.is_empty(), "templates are excluded");
}

// ---------------------------------------------------------------------------
// Gate + malformed input
// ---------------------------------------------------------------------------

#[tokio::test]
async fn player_cannot_read_relationships() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "player").await;

    let status = Client::new()
        .get(format!(
            "{}/campaign/{}/pages/{}/relationships",
            app.base_url,
            campaign_id.0,
            PageId::generate()
        ))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 403);
}

#[tokio::test]
async fn malformed_page_id_is_400() {
    let app = common::spawn_app().await;
    let cid = gm_campaign(&app).await;

    let status = Client::new()
        .get(format!(
            "{}/campaign/{}/pages/not-a-ulid/relationships",
            app.base_url, cid.0
        ))
        .header("authorization", app.auth_header())
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(status, 400);
}
