//! Relationships schema, at the DB layer: the edge round-trips through branded
//! types, `Prior` origin and live state persist as NULL, the knowledge CHECK forbids
//! a reveal on a born-public row, and the PARTIAL unique "live fact" index is live -
//! one live row per canonical fact, while superseded/retconned history coexists. The
//! actor + petgraph and the GM ops are exercised in `relationship_graph_test.rs`;
//! this file pins only the durable schema.

use chrono::{DateTime, Utc};
use familiar_systems_campaign::db;
use familiar_systems_campaign::entities::columns::{
    PageIdCol, PageKindCol, RelationshipIdCol, SessionIdCol, StatusCol,
};
use familiar_systems_campaign::entities::{pages, relationships, sessions};
use familiar_systems_campaign::migrations::Migrator;
use familiar_systems_campaign_shared::id::{PageId, RelationshipId, SessionId};
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, IntoActiveModel, Set};
use sea_orm_migration::MigratorTrait;

async fn setup() -> DatabaseConnection {
    db::register_sqlite_vec();
    let db = db::connect("sqlite::memory:").await.expect("connect");
    Migrator::up(&db, None).await.expect("migrate");
    db
}

/// Seed an entity page (an FK target for relationship endpoints).
async fn seed_page(db: &DatabaseConnection, now: DateTime<Utc>) -> PageId {
    let id = PageId::generate();
    pages::ActiveModel {
        id: Set(PageIdCol::from(id.clone())),
        name: Set("An Entity".into()),
        status: Set(StatusCol::GmOnly),
        kind: Set(PageKindCol::Entity),
        template_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(db)
    .await
    .expect("seed page");
    id
}

/// Seed a temporal session row (an FK target for the four session-stamp axes).
async fn seed_session(db: &DatabaseConnection, now: DateTime<Utc>, ordinal: i64) -> SessionId {
    let id = SessionId::generate();
    sessions::ActiveModel {
        id: Set(SessionIdCol::from(id.clone())),
        ordinal: Set(ordinal),
        created_at: Set(now),
        updated_at: Set(now),
        page_id: Set(None),
    }
    .insert(db)
    .await
    .expect("seed session");
    id
}

/// A live (still-true, not-retconned), `Prior`-origin, born-public relationship.
fn live_rel(
    rel_id: RelationshipId,
    page_a: &PageId,
    page_b: &PageId,
    pred_ab: &str,
    pred_ba: &str,
    now: DateTime<Utc>,
) -> relationships::ActiveModel {
    relationships::ActiveModel {
        id: Set(RelationshipIdCol::from(rel_id)),
        page_a: Set(PageIdCol::from(page_a.clone())),
        page_b: Set(PageIdCol::from(page_b.clone())),
        predicate_a_to_b: Set(pred_ab.to_owned()),
        predicate_b_to_a: Set(pred_ba.to_owned()),
        origin_session_id: Set(None),
        superseded_session_id: Set(None),
        retcon_session_id: Set(None),
        is_secret: Set(false),
        reveal_session_id: Set(None),
        created_at: Set(now),
    }
}

#[tokio::test]
async fn relationship_round_trips_branded_ids_and_both_axes() {
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;
    let origin = seed_session(&db, now, 3).await;
    let revealed = seed_session(&db, now, 5).await;

    let rel_id = RelationshipId::generate();
    relationships::ActiveModel {
        id: Set(RelationshipIdCol::from(rel_id.clone())),
        page_a: Set(PageIdCol::from(page_a.clone())),
        page_b: Set(PageIdCol::from(page_b.clone())),
        predicate_a_to_b: Set("is a resident of".into()),
        predicate_b_to_a: Set("is the home of".into()),
        origin_session_id: Set(Some(SessionIdCol::from(origin.clone()))),
        superseded_session_id: Set(None),
        retcon_session_id: Set(None),
        is_secret: Set(true),
        reveal_session_id: Set(Some(SessionIdCol::from(revealed.clone()))),
        created_at: Set(now),
    }
    .insert(&db)
    .await
    .expect("insert relationship");

    let row = relationships::Entity::find_by_id(RelationshipIdCol::from(rel_id.clone()))
        .one(&db)
        .await
        .expect("query relationship")
        .expect("relationship exists");

    // Typed let-bindings are the assertion: columns must come back branded.
    let id_back: RelationshipId = row.id.into();
    assert_eq!(id_back, rel_id);
    let page_a_back: PageId = row.page_a.into();
    assert_eq!(page_a_back, page_a);
    let origin_back: Option<SessionId> = row.origin_session_id.map(Into::into);
    assert_eq!(origin_back, Some(origin));
    let reveal_back: Option<SessionId> = row.reveal_session_id.map(Into::into);
    assert_eq!(reveal_back, Some(revealed));
    assert!(row.is_secret, "is_secret persists");
    assert_eq!(row.predicate_a_to_b, "is a resident of");
}

#[tokio::test]
async fn prior_origin_live_public_persists_as_nulls() {
    // The critical case: true before session 1, still true, always known. Every
    // session-stamp axis is NULL and is_secret is false.
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;

    let rel_id = RelationshipId::generate();
    live_rel(
        rel_id.clone(),
        &page_a,
        &page_b,
        "is engaged to",
        "is engaged to",
        now,
    )
    .insert(&db)
    .await
    .expect("insert prior-origin live relationship");

    let row = relationships::Entity::find_by_id(RelationshipIdCol::from(rel_id))
        .one(&db)
        .await
        .expect("query")
        .expect("exists");

    assert!(row.origin_session_id.is_none(), "Prior origin is NULL");
    assert!(
        row.superseded_session_id.is_none(),
        "a live relationship is not superseded"
    );
    assert!(
        row.retcon_session_id.is_none(),
        "a live relationship is not retconned"
    );
    assert!(!row.is_secret, "born public");
    assert!(
        row.reveal_session_id.is_none(),
        "no reveal on a public fact"
    );
}

#[tokio::test]
async fn knowledge_check_rejects_public_with_reveal() {
    // The CHECK `is_secret OR reveal_session_id IS NULL`: a born-public row may not
    // carry a reveal event.
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;
    let revealed = seed_session(&db, now, 4).await;

    let mut illegal = live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "knows",
        "knows",
        now,
    );
    illegal.is_secret = Set(false);
    illegal.reveal_session_id = Set(Some(SessionIdCol::from(revealed)));

    assert!(
        illegal.insert(&db).await.is_err(),
        "a public fact carrying a reveal must violate the knowledge CHECK"
    );
}

#[tokio::test]
async fn live_duplicate_canonical_fact_is_rejected() {
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;

    live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "rules",
        "is ruled by",
        now,
    )
    .insert(&db)
    .await
    .expect("first live row inserts");

    // A second LIVE row with the same canonical (pair + predicate pair) must
    // violate the partial unique index.
    let dup = live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "rules",
        "is ruled by",
        now,
    )
    .insert(&db)
    .await;

    assert!(
        dup.is_err(),
        "two live rows for one canonical fact must be rejected"
    );
}

#[tokio::test]
async fn superseded_row_coexists_with_new_live_row() {
    // Supersede in miniature: end the live row (set superseded), then a new live row
    // with the same predicate pair inserts cleanly because the partial index only
    // constrains the live set.
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;
    let ender = seed_session(&db, now, 12).await;

    let first = live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "allied with",
        "allied with",
        now,
    )
    .insert(&db)
    .await
    .expect("first live row");

    let mut ended = first.into_active_model();
    ended.superseded_session_id = Set(Some(SessionIdCol::from(ender)));
    ended.update(&db).await.expect("end the first row");

    // A fresh LIVE row with the identical predicate pair now inserts.
    live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "allied with",
        "allied with",
        now,
    )
    .insert(&db)
    .await
    .expect("a new live row coexists with the superseded one");
}

#[tokio::test]
async fn retconned_row_coexists_with_new_live_row() {
    // A retconned row also drops out of the live index (liveness is factuality only:
    // neither superseded nor retconned), so a fresh live row with the same pair
    // inserts alongside it.
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;
    let caught = seed_session(&db, now, 2).await;

    let first = live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "is sworn to",
        "is liege of",
        now,
    )
    .insert(&db)
    .await
    .expect("first live row");

    let mut retconned = first.into_active_model();
    retconned.retcon_session_id = Set(Some(SessionIdCol::from(caught)));
    retconned.update(&db).await.expect("retcon the first row");

    live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "is sworn to",
        "is liege of",
        now,
    )
    .insert(&db)
    .await
    .expect("a live row coexists with a retconned one");
}
