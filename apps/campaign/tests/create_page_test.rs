mod common;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::registry::{CreateCampaign, resolve};
use familiar_systems_campaign::db::connect_readonly;
use familiar_systems_campaign::entities::blocks;
use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::loro::page::Section;
use sea_orm::{DatabaseConnection, EntityTrait};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Duration;
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{method, path},
};

async fn create_campaign(app: &common::TestApp, campaign_id: &CampaignId) {
    let state = app
        .registry
        .ask(CreateCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: common::test_user_id(),
        })
        .await
        .expect("create campaign");
    resolve(Some(state), Duration::from_secs(30))
        .await
        .expect("campaign ready");
}

/// Mount the platform membership endpoint for this campaign+test-user, with
/// the given functional role (`"gm"` or `"player"`).
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

/// POST a kind-tagged `{ kind, content }` page body and return the HTTP status.
async fn post_page(app: &common::TestApp, campaign_id: &CampaignId, body: Value) -> u16 {
    reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&body)
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn gm_creates_page_and_nests_under_parent() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let client = reqwest::Client::new();
    let url = format!("{}/campaign/{}/pages", app.base_url, campaign_id.0);

    // Create a root-level entity. The request and response are kind-tagged:
    // `{ kind, content: { ... } }`.
    let resp = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Korgath" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["kind"], "entity");
    assert_eq!(body["content"]["name"], "Korgath");
    assert_eq!(body["content"]["status"], "gmOnly", "defaults to gm_only");
    assert!(body["content"]["template_id"].is_null());
    let parent_id = body["content"]["id"]
        .as_str()
        .expect("id is a string")
        .to_string();

    // Create a child nested under the first entity.
    let resp = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Korgath's Lair", "parent": parent_id } }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status().as_u16(),
        201,
        "nesting under a real parent succeeds"
    );
}

#[tokio::test]
async fn gm_creates_template() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "template", "content": { "name": "NPC" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["kind"], "template");
    assert_eq!(body["content"]["name"], "NPC");
    // A template carries no `template_id` lineage of its own; the response
    // variant has no such field.
    assert!(body["content"]["template_id"].is_null());
    assert!(body["content"]["id"].as_str().is_some());
}

#[tokio::test]
async fn gm_creates_session_via_pages() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    // A session is named like every other page kind.
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "session", "content": { "name": "The End of Perth" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);

    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["kind"], "session");
    // The session response carries the temporal record the page form does not.
    assert_eq!(
        body["content"]["ordinal"], 1,
        "first session in the campaign"
    );
    assert_eq!(body["content"]["name"], "The End of Perth");
    assert!(body["content"]["page_id"].as_str().is_some());
    assert!(body["content"]["session_id"].as_str().is_some());
}

/// A blank session name is rejected (422), like every other page kind - a session
/// is no longer the unnamed exception.
#[tokio::test]
async fn creating_a_blank_session_is_rejected() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let status = post_page(
        &app,
        &campaign_id,
        json!({ "kind": "session", "content": { "name": "   " } }),
    )
    .await;
    assert_eq!(status, 422, "a whitespace-only session name is rejected");
}

/// Names are unique per kind: a second page of the same kind with the same name
/// is a 409, but the same name on a *different* kind is fine - "The Fall of
/// Perth" can be both an entity and a session.
#[tokio::test]
async fn duplicate_name_is_rejected_per_kind() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let entity = |name: &str| json!({ "kind": "entity", "content": { "name": name } });
    let session = |name: &str| json!({ "kind": "session", "content": { "name": name } });

    assert_eq!(
        post_page(&app, &campaign_id, entity("The Fall of Perth")).await,
        201,
        "first entity with the name is created"
    );
    assert_eq!(
        post_page(&app, &campaign_id, entity("The Fall of Perth")).await,
        409,
        "a second entity with the same name collides"
    );
    assert_eq!(
        post_page(&app, &campaign_id, session("The Fall of Perth")).await,
        201,
        "the same name on a different kind (session) is allowed"
    );
    assert_eq!(
        post_page(&app, &campaign_id, session("The Fall of Perth")).await,
        409,
        "a second session with the same name collides"
    );
}

#[tokio::test]
async fn create_page_with_unknown_parent_returns_422() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Orphan", "parent": PageId::generate().to_string() } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 422);
}

#[tokio::test]
async fn player_cannot_create_page() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "player").await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Forbidden" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 403);
}

#[tokio::test]
async fn create_page_unknown_campaign_returns_404() {
    let app = common::spawn_app().await;

    // Not checked out on this shard -> 404 (after the body parses but before the
    // membership check).
    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/nonexistent-id/pages", app.base_url))
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Ghost" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 404);
}

/// Read a page's persisted blocks straight from the campaign SQLite. Genesis
/// commits synchronously before its HTTP 201, so a create's rows are readable the
/// instant it returns - no flush needed.
async fn page_blocks(db: &DatabaseConnection, page_id: &str) -> Vec<blocks::Model> {
    blocks::Entity::find()
        .all(db)
        .await
        .expect("query blocks")
        .into_iter()
        .filter(|b| PageId::from(b.page_id.clone()).to_string() == page_id)
        .collect()
}

/// Cloning an entity from a template deep-copies the template's blocks with fresh
/// ids and stamps the lineage. A template made via `POST` carries the two seeded
/// (preamble + body) paragraphs, so the clone must carry two too, all new ids.
#[tokio::test]
async fn create_entity_from_template_clones_blocks_with_fresh_ids() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let client = reqwest::Client::new();
    let url = format!("{}/campaign/{}/pages", app.base_url, campaign_id.0);

    // A template to clone from.
    let tmpl: Value = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "template", "content": { "name": "NPC" } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let template_id = tmpl["content"]["id"].as_str().unwrap().to_string();

    // Clone it into an entity.
    let resp = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({
            "kind": "entity",
            "content": { "name": "Grimhollow", "from_template_id": template_id }
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 201);
    let entity: Value = resp.json().await.unwrap();
    assert_eq!(
        entity["content"]["template_id"], template_id,
        "the response echoes the cloned-from lineage"
    );
    let entity_id = entity["content"]["id"].as_str().unwrap().to_string();

    let db_path = app.data_dir.path().join(format!("{}.db", campaign_id.0));
    let db = connect_readonly(&db_path).await.expect("open campaign db");
    let template_blocks = page_blocks(&db, &template_id).await;
    let entity_blocks = page_blocks(&db, &entity_id).await;

    assert!(
        !template_blocks.is_empty(),
        "the template has seeded blocks to clone"
    );
    assert_eq!(
        entity_blocks.len(),
        template_blocks.len(),
        "the clone has one block per template block"
    );

    // Sections carry over (as a multiset).
    let sections = |bs: &[blocks::Model]| {
        let mut s: Vec<Section> = bs.iter().map(|b| Section::from(b.section)).collect();
        s.sort_by_key(|s| s.as_str());
        s
    };
    assert_eq!(
        sections(&entity_blocks),
        sections(&template_blocks),
        "cloned blocks keep their sections"
    );

    // Every cloned block id is fresh: none collides with a template block id.
    let template_ids: HashSet<BlockId> = template_blocks
        .iter()
        .map(|b| BlockId::from(b.id.clone()))
        .collect();
    assert!(
        entity_blocks
            .iter()
            .all(|b| !template_ids.contains(&BlockId::from(b.id.clone()))),
        "cloned block ids are freshly minted, never shared with the template"
    );
}

/// `from_template_id` must reference a real template. A random id (no such page)
/// and a real *non-template* page id both fail with 422.
#[tokio::test]
async fn cloning_from_a_non_template_returns_422() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;
    mount_membership(&app, &campaign_id, "gm").await;

    let client = reqwest::Client::new();
    let url = format!("{}/campaign/{}/pages", app.base_url, campaign_id.0);

    // Unknown id.
    assert_eq!(
        post_page(
            &app,
            &campaign_id,
            json!({ "kind": "entity", "content": { "name": "Ghost", "from_template_id": PageId::generate().to_string() } }),
        )
        .await,
        422,
        "an unknown from_template_id is rejected"
    );

    // A real page that is an entity, not a template.
    let entity: Value = client
        .post(&url)
        .header("authorization", app.auth_header())
        .json(&json!({ "kind": "entity", "content": { "name": "Not A Template" } }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let entity_id = entity["content"]["id"].as_str().unwrap().to_string();

    assert_eq!(
        post_page(
            &app,
            &campaign_id,
            json!({ "kind": "entity", "content": { "name": "Cloned", "from_template_id": entity_id } }),
        )
        .await,
        422,
        "cloning from a non-template page is rejected"
    );
}

#[tokio::test]
async fn create_page_without_auth_returns_401() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    create_campaign(&app, &campaign_id).await;

    let resp = reqwest::Client::new()
        .post(format!("{}/campaign/{}/pages", app.base_url, campaign_id.0))
        .json(&json!({ "kind": "entity", "content": { "name": "Anon" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status().as_u16(), 401);
}
