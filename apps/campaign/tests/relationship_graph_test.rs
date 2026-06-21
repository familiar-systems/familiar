//! `RelationshipGraph` actor integration: the five ops + reads, driven by `ask`
//! against a real in-memory SQLite + `DatabaseWriteActor`. Asserts the actor
//! validates + canonicalizes, composes ops from single-statement writer primitives
//! (supersede = create-then-invalidate), orients edges per page, and rebuilds its
//! petgraph from the table on restart.

use chrono::Utc;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::database_writer::{
    DatabaseWriteActor, DatabaseWriteActorArgs,
};
use familiar_systems_campaign::actors::relationship_graph::{
    ApplyOp, ApplyOpError, CreateRelationship, CreateRelationshipError, KnownPredicatePairs,
    RelationshipGraph, RelationshipGraphArgs, RelationshipOp, RelationshipsForPage,
};
use familiar_systems_campaign::db;
use familiar_systems_campaign::domain::relationship::{Ending, Origin};
use familiar_systems_campaign::entities::columns::{
    PageIdCol, PageKindCol, RelationshipIdCol, SessionIdCol, StatusCol,
};
use familiar_systems_campaign::entities::{pages, relationships, sessions};
use familiar_systems_campaign::migrations::Migrator;
use familiar_systems_campaign_shared::id::{PageId, SessionId};
use familiar_systems_campaign_shared::relationship::{
    InvalidationReason, ViewInvalidation, ViewSessionPoint, Visibility,
};
use kameo::actor::{ActorRef, Spawn};
use kameo::error::SendError;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
use sea_orm_migration::MigratorTrait;

async fn setup() -> DatabaseConnection {
    db::register_sqlite_vec();
    let db = db::connect("sqlite::memory:").await.expect("connect");
    Migrator::up(&db, None).await.expect("migrate");
    db
}

async fn spawn_graph(
    conn: &DatabaseConnection,
) -> (ActorRef<DatabaseWriteActor>, ActorRef<RelationshipGraph>) {
    let campaign_id = CampaignId::generate();
    let writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: campaign_id.clone(),
        conn: conn.clone(),
    });
    let graph = RelationshipGraph::spawn(RelationshipGraphArgs {
        campaign_id,
        db_reader: conn.clone(),
        db_writer: writer.clone(),
    });
    graph.wait_for_startup().await;
    (writer, graph)
}

async fn seed_page(conn: &DatabaseConnection, name: &str) -> PageId {
    let id = PageId::generate();
    pages::ActiveModel {
        id: Set(PageIdCol::from(id.clone())),
        name: Set(name.into()),
        status: Set(StatusCol::GmOnly),
        kind: Set(PageKindCol::Entity),
        template_id: Set(None),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
    }
    .insert(conn)
    .await
    .expect("seed page");
    id
}

async fn seed_session(conn: &DatabaseConnection, ordinal: i64) -> SessionId {
    let id = SessionId::generate();
    sessions::ActiveModel {
        id: Set(SessionIdCol::from(id.clone())),
        ordinal: Set(ordinal),
        created_at: Set(Utc::now()),
        updated_at: Set(Utc::now()),
        page_id: Set(None),
    }
    .insert(conn)
    .await
    .expect("seed session");
    id
}

/// A live create with sensible defaults (Players-visible, Prior origin, no ending).
fn create(subject: &PageId, other: &PageId, fwd: &str, rev: &str) -> CreateRelationship {
    CreateRelationship {
        subject: subject.clone(),
        other: other.clone(),
        predicate_forward: fwd.into(),
        predicate_reverse: rev.into(),
        visibility: Visibility::Players,
        origin: Origin::Prior,
        ending: None,
        supersedes: None,
    }
}

/// The ordinal of a `Session` origin (panics on `Prior` - callers expect a session).
fn session_origin_ordinal(origin: &ViewSessionPoint) -> i64 {
    match origin {
        ViewSessionPoint::Session(o) => o.ordinal,
        ViewSessionPoint::Prior => panic!("expected Session origin, got Prior"),
    }
}

/// The ordinal a `Superseded` invalidation ended at (panics on `Retconned` or a
/// `Prior` end - callers expect a session-dated supersession).
fn ended_session_ordinal(inv: &ViewInvalidation) -> i64 {
    match inv {
        ViewInvalidation::Superseded {
            ended: ViewSessionPoint::Session(o),
        } => o.ordinal,
        other => panic!("expected Superseded-at-session, got {other:?}"),
    }
}

#[tokio::test]
async fn create_persists_and_orients_from_both_endpoints() {
    let conn = setup().await;
    let john = seed_page(&conn, "John").await;
    let town = seed_page(&conn, "Townsville").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(create(&john, &town, "is a resident of", "is the home of"))
        .await
        .expect("create");
    assert_eq!(view.other.id, town);
    assert_eq!(view.other.name, "Townsville");
    assert_eq!(view.predicate, "is a resident of");
    assert_eq!(view.predicate_reverse, "is the home of");
    assert!(matches!(view.origin, ViewSessionPoint::Prior));
    assert!(view.invalidation.is_none());

    let from_john = graph
        .ask(RelationshipsForPage {
            page_id: john.clone(),
        })
        .await
        .expect("for john");
    assert_eq!(from_john.len(), 1);
    assert_eq!(from_john[0].predicate, "is a resident of");
    assert_eq!(from_john[0].other.name, "Townsville");

    // The far page reads the same edge reversed.
    let from_town = graph
        .ask(RelationshipsForPage {
            page_id: town.clone(),
        })
        .await
        .expect("for town");
    assert_eq!(from_town.len(), 1);
    assert_eq!(from_town[0].predicate, "is the home of");
    assert_eq!(from_town[0].other.name, "John");
}

#[tokio::test]
async fn create_stores_canonically_regardless_of_subject_order() {
    let conn = setup().await;
    let p1 = seed_page(&conn, "One").await;
    let p2 = seed_page(&conn, "Two").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    // Create from the lexicographically larger page, so canonicalize must swap.
    let (subject, other) = if p1.0 > p2.0 {
        (p1.clone(), p2.clone())
    } else {
        (p2.clone(), p1.clone())
    };
    graph
        .ask(create(
            &subject,
            &other,
            "owes a debt to",
            "holds marker on",
        ))
        .await
        .expect("create");

    let rows = relationships::Entity::find().all(&conn).await.unwrap();
    assert_eq!(rows.len(), 1);
    let a: PageId = rows[0].page_a.clone().into();
    let b: PageId = rows[0].page_b.clone().into();
    assert!(a.0 < b.0, "page_a is stored as the smaller PageId");

    // Orientation from the subject still reads the forward we gave.
    let view = graph
        .ask(RelationshipsForPage {
            page_id: subject.clone(),
        })
        .await
        .unwrap();
    assert_eq!(view[0].predicate, "owes a debt to");
    assert_eq!(view[0].other.id, other);
}

#[tokio::test]
async fn create_rejects_self_edge_and_empty_predicate() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let self_edge = graph.ask(create(&a, &a, "knows", "knows")).await;
    assert!(matches!(
        self_edge,
        Err(SendError::HandlerError(CreateRelationshipError::SelfEdge))
    ));

    let empty = graph.ask(create(&a, &b, "  ", "x")).await;
    assert!(matches!(
        empty,
        Err(SendError::HandlerError(
            CreateRelationshipError::EmptyPredicate
        ))
    ));
}

#[tokio::test]
async fn create_rejects_missing_page() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let ghost = PageId::generate(); // never seeded
    let (_writer, graph) = spawn_graph(&conn).await;

    let res = graph.ask(create(&a, &ghost, "knows", "is known by")).await;
    assert!(matches!(
        res,
        Err(SendError::HandlerError(
            CreateRelationshipError::PageNotFound(_)
        ))
    ));
}

#[tokio::test]
async fn duplicate_live_fact_is_rejected() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    graph
        .ask(create(&a, &b, "rules", "is ruled by"))
        .await
        .expect("first create");
    let dup = graph.ask(create(&a, &b, "rules", "is ruled by")).await;
    assert!(matches!(
        dup,
        Err(SendError::HandlerError(
            CreateRelationshipError::DuplicateLiveFact
        ))
    ));
    assert_eq!(
        relationships::Entity::find()
            .all(&conn)
            .await
            .unwrap()
            .len(),
        1,
        "the rejected create left no row"
    );
}

#[tokio::test]
async fn end_invalidates_but_keeps_it_in_the_curation_view() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s12 = seed_session(&conn, 12).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(create(&a, &b, "is captain of", "is captained by"))
        .await
        .expect("create");
    graph
        .ask(ApplyOp {
            rel_id: view.id.clone(),
            op: RelationshipOp::End { as_of: s12 },
        })
        .await
        .expect("end");

    let from_a = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert_eq!(from_a.len(), 1, "still shown in the GM curation view");
    let inv = from_a[0].invalidation.as_ref().expect("invalidated");
    assert_eq!(
        ended_session_ordinal(inv),
        12,
        "ended at S12 (reason superseded)"
    );

    let row = relationships::Entity::find_by_id(RelationshipIdCol::from(view.id))
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(row.invalidation_reason.is_some(), "DB row is invalidated");
}

#[tokio::test]
async fn supersede_ends_old_and_creates_new() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s6 = seed_session(&conn, 6).await;
    let s12 = seed_session(&conn, 12).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let original = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "is captain of".into(),
            predicate_reverse: "is captained by".into(),
            visibility: Visibility::Players,
            origin: Origin::Session(s6),
            ending: None,
            supersedes: None,
        })
        .await
        .expect("create");

    // Supersede = a create that points at the row it replaces; the old is ended in
    // the same transaction, and the reply is the new row's view.
    let replacement = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "is quartermaster of".into(),
            predicate_reverse: "is quartermastered by".into(),
            visibility: Visibility::Players,
            origin: Origin::Session(s12),
            ending: None,
            supersedes: Some(original.id.clone()),
        })
        .await
        .expect("supersede");
    assert_eq!(replacement.predicate, "is quartermaster of");
    assert!(
        replacement.invalidation.is_none(),
        "the reply is the live new row"
    );
    assert_eq!(session_origin_ordinal(&replacement.origin), 12);

    let from_a = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert_eq!(from_a.len(), 2, "old (invalidated) + new (live)");

    let old = from_a
        .iter()
        .find(|v| v.id == original.id)
        .expect("old present");
    assert_eq!(old.predicate, "is captain of", "old predicate is immutable");
    assert_eq!(
        ended_session_ordinal(old.invalidation.as_ref().expect("old invalidated")),
        12
    );

    let new = from_a
        .iter()
        .find(|v| v.id != original.id)
        .expect("new present");
    assert_eq!(new.predicate, "is quartermaster of");
    assert!(new.invalidation.is_none(), "new row is live");
    assert_eq!(
        session_origin_ordinal(&new.origin),
        12,
        "new fact originates at the supersede session"
    );
}

/// Supersede is an atomic Create+Invalidate batch: the "wait, they're already
/// divorced" case. When the new fact already exists live, create-first trips the
/// partial unique index on the insert and rolls the whole batch back - so the old
/// stays live, no orphan row, and the GM can fall back to a plain End.
#[tokio::test]
async fn supersede_rejecting_a_duplicate_new_fact_leaves_the_old_live() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s12 = seed_session(&conn, 12).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    // R1: the live fact the superseding new row will collide with.
    graph
        .ask(create(&a, &b, "rules", "is ruled by"))
        .await
        .expect("R1");
    // R2: a different live pair, coexisting with R1.
    let r2 = graph
        .ask(create(&a, &b, "guards", "is guarded by"))
        .await
        .expect("R2");

    let res = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "rules".into(),
            predicate_reverse: "is ruled by".into(),
            visibility: Visibility::Players,
            origin: Origin::Session(s12),
            ending: None,
            supersedes: Some(r2.id.clone()),
        })
        .await;
    assert!(matches!(
        res,
        Err(SendError::HandlerError(
            CreateRelationshipError::DuplicateLiveFact
        ))
    ));

    // The create tripped the unique index and rolled the batch back: R2 untouched,
    // no new row.
    let rows = relationships::Entity::find().all(&conn).await.unwrap();
    assert_eq!(rows.len(), 2, "no third row was created");
    let r2_row = relationships::Entity::find_by_id(RelationshipIdCol::from(r2.id))
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(
        r2_row.invalidation_reason.is_none(),
        "R2 was never invalidated; it stays live"
    );
}

/// A relationship can be born already finalized - a retrofit/correction ("they were
/// married S3, divorced S6", entered now). It persists with both an origin and an
/// invalidation, and sits outside the live set (so a fresh live row with the same
/// pair coexists).
#[tokio::test]
async fn create_can_birth_a_finalized_relationship() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s3 = seed_session(&conn, 3).await;
    let s6 = seed_session(&conn, 6).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "is married to".into(),
            predicate_reverse: "is married to".into(),
            visibility: Visibility::Players,
            origin: Origin::Session(s3),
            ending: Some(Ending {
                reason: InvalidationReason::Superseded,
                by: Origin::Session(s6),
            }),
            supersedes: None,
        })
        .await
        .expect("create finalized");

    // Born with both an origin (S3) and an invalidation (ended S6).
    assert_eq!(session_origin_ordinal(&view.origin), 3);
    let inv = view.invalidation.as_ref().expect("born finalized");
    assert_eq!(
        ended_session_ordinal(inv),
        6,
        "born ended at S6 (superseded)"
    );

    // It's outside the live set, so a fresh live row with the same pair coexists.
    graph
        .ask(create(&a, &b, "is married to", "is married to"))
        .await
        .expect("a live row coexists with the finalized one");
    assert_eq!(
        relationships::Entity::find()
            .all(&conn)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn retcon_marks_reason_retconned() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let v = graph
        .ask(create(&a, &b, "is brother to", "is brother to"))
        .await
        .expect("create");
    graph
        .ask(ApplyOp {
            rel_id: v.id.clone(),
            op: RelationshipOp::Retcon,
        })
        .await
        .expect("retcon");

    let from_a = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert!(
        matches!(
            from_a[0]
                .invalidation
                .as_ref()
                .expect("retconned is invalidated"),
            ViewInvalidation::Retconned
        ),
        "retcon surfaces as the Retconned variant (no ordinal)",
    );
}

#[tokio::test]
async fn delete_removes_from_store_and_db() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let v = graph
        .ask(create(&a, &b, "owes a debt to", "holds marker on"))
        .await
        .expect("create");
    graph
        .ask(ApplyOp {
            rel_id: v.id,
            op: RelationshipOp::Delete,
        })
        .await
        .expect("delete");

    assert!(
        graph
            .ask(RelationshipsForPage { page_id: a.clone() })
            .await
            .unwrap()
            .is_empty(),
        "gone from the in-memory graph"
    );
    assert_eq!(
        relationships::Entity::find()
            .all(&conn)
            .await
            .unwrap()
            .len(),
        0,
        "hard-deleted from the table"
    );
}

#[tokio::test]
async fn set_visibility_updates_without_invalidating() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let v = graph
        .ask(create(&a, &b, "is suspicious of", "is distrusted by"))
        .await
        .expect("create");
    graph
        .ask(ApplyOp {
            rel_id: v.id,
            op: RelationshipOp::SetVisibility {
                visibility: Visibility::Gm,
            },
        })
        .await
        .expect("set visibility");

    let from_a = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert_eq!(from_a[0].visibility, Visibility::Gm);
    assert!(
        from_a[0].invalidation.is_none(),
        "a visibility change does not invalidate"
    );
}

#[tokio::test]
async fn known_predicate_pairs_clusters_with_counts() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let c = seed_page(&conn, "C").await;
    let (_writer, graph) = spawn_graph(&conn).await;

    // Same pair on two edges, a different pair on a third.
    graph
        .ask(create(&a, &b, "is a resident of", "is the home of"))
        .await
        .expect("e1");
    graph
        .ask(create(&a, &c, "is a resident of", "is the home of"))
        .await
        .expect("e2");
    graph
        .ask(create(&b, &c, "is allied with", "is allied with"))
        .await
        .expect("e3");

    let mut pairs = graph.ask(KnownPredicatePairs).await.unwrap();
    pairs.sort_by_key(|p| std::cmp::Reverse(p.count));
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].count, 2, "the resident/home pair clusters");
    assert_eq!(pairs[1].count, 1);
    assert_eq!(pairs[1].forward, "is allied with");
}

#[tokio::test]
async fn restart_reloads_graph_from_table() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let (writer, graph) = spawn_graph(&conn).await;

    graph
        .ask(create(&a, &b, "keeps the key to", "is kept by"))
        .await
        .expect("create");

    // A fresh graph over the same table must rebuild its petgraph on start.
    let graph2 = RelationshipGraph::spawn(RelationshipGraphArgs {
        campaign_id: CampaignId::generate(),
        db_reader: conn.clone(),
        db_writer: writer.clone(),
    });
    graph2.wait_for_startup().await;

    let from_a = graph2
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert_eq!(from_a.len(), 1, "reloaded from the table");
    assert_eq!(from_a[0].predicate, "keeps the key to");
}

#[tokio::test]
async fn end_on_an_ended_row_is_rejected() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s12 = seed_session(&conn, 12).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(create(&a, &b, "is captain of", "is captained by"))
        .await
        .expect("create");
    graph
        .ask(ApplyOp {
            rel_id: view.id.clone(),
            op: RelationshipOp::End { as_of: s12.clone() },
        })
        .await
        .expect("end");

    // The one-way door: a second End on the same row is rejected, not silently reapplied.
    let res = graph
        .ask(ApplyOp {
            rel_id: view.id,
            op: RelationshipOp::End { as_of: s12 },
        })
        .await;
    assert!(matches!(
        res,
        Err(SendError::HandlerError(ApplyOpError::AlreadyInvalidated))
    ));
}

#[tokio::test]
async fn end_before_origin_is_rejected() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s6 = seed_session(&conn, 6).await;
    let s3 = seed_session(&conn, 3).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    // Born at S6, then asked to end as of the earlier S3: a fact cannot end before it began.
    let view = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "is captain of".into(),
            predicate_reverse: "is captained by".into(),
            visibility: Visibility::Players,
            origin: Origin::Session(s6),
            ending: None,
            supersedes: None,
        })
        .await
        .expect("create");

    let res = graph
        .ask(ApplyOp {
            rel_id: view.id,
            op: RelationshipOp::End { as_of: s3 },
        })
        .await;
    assert!(matches!(
        res,
        Err(SendError::HandlerError(ApplyOpError::EndBeforeOrigin))
    ));
}
