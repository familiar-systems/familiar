//! Relationships schema, at the DB layer: the edge round-trips through branded
//! types and the enum boundary, `Prior` origin and live state both persist as
//! NULL, and the PARTIAL unique "live fact" index is live - one live row per
//! canonical fact, while invalidated history (including the ended-before-the-
//! campaign case) coexists. The five GM ops and the in-memory petgraph land with
//! their actor in Slice 2; this file pins only the durable schema.

use chrono::{DateTime, Utc};
use familiar_systems_campaign::db;
use familiar_systems_campaign::entities::columns::{
    InvalidationReasonCol, PageIdCol, PageKindCol, RelationshipIdCol, SessionIdCol, StatusCol,
    VisibilityCol,
};
use familiar_systems_campaign::entities::{pages, relationships, sessions};
use familiar_systems_campaign::migrations::Migrator;
use familiar_systems_campaign_shared::id::{PageId, RelationshipId, SessionId};
use familiar_systems_campaign_shared::relationship::Visibility;
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

/// Seed a temporal session row (an FK target for origin / invalidated_by).
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

/// A live (un-invalidated), `Prior`-origin relationship between two pages.
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
        visibility: Set(VisibilityCol::Players),
        origin_session_id: Set(None),
        created_at: Set(now),
        invalidation_reason: Set(None),
        invalidated_by_session_id: Set(None),
        invalidated_at: Set(None),
    }
}

#[tokio::test]
async fn relationship_round_trips_branded_ids_and_enums() {
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;
    let origin = seed_session(&db, now, 3).await;

    let rel_id = RelationshipId::generate();
    relationships::ActiveModel {
        id: Set(RelationshipIdCol::from(rel_id.clone())),
        page_a: Set(PageIdCol::from(page_a.clone())),
        page_b: Set(PageIdCol::from(page_b.clone())),
        predicate_a_to_b: Set("is a resident of".into()),
        predicate_b_to_a: Set("is the home of".into()),
        visibility: Set(VisibilityCol::Players),
        origin_session_id: Set(Some(SessionIdCol::from(origin.clone()))),
        created_at: Set(now),
        invalidation_reason: Set(None),
        invalidated_by_session_id: Set(None),
        invalidated_at: Set(None),
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
    let visibility_back: Visibility = row.visibility.into();
    assert_eq!(visibility_back, Visibility::Players);
    let origin_back: Option<SessionId> = row.origin_session_id.map(Into::into);
    assert_eq!(origin_back, Some(origin));
    assert_eq!(row.predicate_a_to_b, "is a resident of");
}

#[tokio::test]
async fn prior_origin_still_live_persists_as_nulls() {
    // The critical case: true before session 1, still true now. Both axes NULL.
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

    assert!(
        row.origin_session_id.is_none(),
        "Prior origin persists as NULL"
    );
    assert!(
        row.invalidation_reason.is_none(),
        "a live relationship has no invalidation reason"
    );
    assert!(row.invalidated_by_session_id.is_none());
    assert!(row.invalidated_at.is_none());
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
async fn invalidated_row_coexists_with_new_live_row() {
    // Supersede in miniature: end the live row, then a new live row with the same
    // predicate pair inserts cleanly because the partial index only constrains
    // the live set.
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

    // Mark it invalidated (reason is the live/invalidated discriminant).
    let mut ended = first.into_active_model();
    ended.invalidation_reason = Set(Some(InvalidationReasonCol::Superseded));
    ended.invalidated_by_session_id = Set(Some(SessionIdCol::from(ender)));
    ended.invalidated_at = Set(Some(now));
    ended.update(&db).await.expect("invalidate the first row");

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
    .expect("a new live row coexists with the invalidated one");
}

#[tokio::test]
async fn ended_in_prior_time_persists_and_is_excluded_from_live() {
    // Case 2 (UI-deferred but encodable): a relationship that ended before the
    // campaign began - reason set, but no invalidating session.
    let db = setup().await;
    let now = Utc::now();

    let page_a = seed_page(&db, now).await;
    let page_b = seed_page(&db, now).await;

    let rel_id = RelationshipId::generate();
    let mut ended_in_prior = live_rel(
        rel_id.clone(),
        &page_a,
        &page_b,
        "was betrothed to",
        "was betrothed to",
        now,
    );
    ended_in_prior.invalidation_reason = Set(Some(InvalidationReasonCol::Superseded));
    ended_in_prior.invalidated_by_session_id = Set(None); // ended in prior time
    ended_in_prior.invalidated_at = Set(Some(now));
    ended_in_prior
        .insert(&db)
        .await
        .expect("ended-in-prior row persists");

    let row = relationships::Entity::find_by_id(RelationshipIdCol::from(rel_id))
        .one(&db)
        .await
        .expect("query")
        .expect("exists");
    assert!(row.invalidation_reason.is_some(), "it is invalidated");
    assert!(
        row.invalidated_by_session_id.is_none(),
        "no session ended it (prior time)"
    );

    // It's outside the live set, so a fresh live row with the same pair inserts.
    live_rel(
        RelationshipId::generate(),
        &page_a,
        &page_b,
        "was betrothed to",
        "was betrothed to",
        now,
    )
    .insert(&db)
    .await
    .expect("a live row coexists with an ended-in-prior row");
}
