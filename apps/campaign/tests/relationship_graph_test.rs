//! `RelationshipGraph` actor integration: create / patch (the reversible two-axis
//! ops) / delete + reads, driven by `ask` against a real in-memory SQLite +
//! `DatabaseWriteActor`. Asserts the actor validates + canonicalizes, composes ops
//! from single-statement writer primitives (supersede = create-then-end), orients
//! both axes per page, enforces the ordering + born-secret invariants, and rebuilds
//! its petgraph from the table on restart.

use chrono::Utc;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::database_writer::{
    DatabaseWriteActor, DatabaseWriteActorArgs,
};
use familiar_systems_campaign::actors::relationship_graph::{
    CreateRelationship, CreateRelationshipError, DeleteRelationship, KnownPredicatePairs,
    PatchRelationship, PatchRelationshipError, RelationshipGraph, RelationshipGraphArgs,
    RelationshipsForPage, StampChange,
};
use familiar_systems_campaign::db;
use familiar_systems_campaign::domain::relationship::{Knowledge, Origin};
use familiar_systems_campaign::entities::columns::{
    PageIdCol, PageKindCol, RelationshipIdCol, SessionIdCol, StatusCol,
};
use familiar_systems_campaign::entities::{pages, relationships, sessions};
use familiar_systems_campaign::migrations::Migrator;
use familiar_systems_campaign_shared::id::{PageId, RelationshipId, SessionId};
use familiar_systems_campaign_shared::relationship::{KnowledgeView, ViewSessionPoint};
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

/// A live, born-public, `Prior`-origin create.
fn create(subject: &PageId, other: &PageId, fwd: &str, rev: &str) -> CreateRelationship {
    CreateRelationship {
        subject: subject.clone(),
        other: other.clone(),
        predicate_forward: fwd.into(),
        predicate_reverse: rev.into(),
        origin: Origin::Prior,
        knowledge: Knowledge::Public,
        supersedes: None,
    }
}

/// A patch that touches only one axis (the other two unchanged).
fn patch_superseded(rel_id: &RelationshipId, change: StampChange) -> PatchRelationship {
    PatchRelationship {
        rel_id: rel_id.clone(),
        knowledge: None,
        superseded: Some(change),
        retcon: None,
    }
}
fn patch_knowledge(rel_id: &RelationshipId, knowledge: Knowledge) -> PatchRelationship {
    PatchRelationship {
        rel_id: rel_id.clone(),
        knowledge: Some(knowledge),
        superseded: None,
        retcon: None,
    }
}
fn patch_retcon(rel_id: &RelationshipId, change: StampChange) -> PatchRelationship {
    PatchRelationship {
        rel_id: rel_id.clone(),
        knowledge: None,
        superseded: None,
        retcon: Some(change),
    }
}

/// The ordinal of a `Session` origin (panics on `Prior` - callers expect a session).
fn session_origin_ordinal(origin: &ViewSessionPoint) -> i64 {
    match origin {
        ViewSessionPoint::Session(o) => o.ordinal,
        ViewSessionPoint::Prior => panic!("expected Session origin, got Prior"),
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
    assert!(view.superseded.is_none());
    assert!(view.retcon.is_none());
    assert!(matches!(view.knowledge, KnowledgeView::Public));

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

/// A born-revealed, session-origin create returns a view whose origin and reveal
/// ordinals come from the reads done during validation: the response is built from
/// data already in hand, never re-read after the commit. Regression for the
/// "committed create returns a transient 500, the retry then 409s" path.
#[tokio::test]
async fn create_born_revealed_orients_from_validation_reads() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s3 = seed_session(&conn, 3).await;
    let s7 = seed_session(&conn, 7).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "betrayed".into(),
            predicate_reverse: "was betrayed by".into(),
            origin: Origin::Session(s3),
            knowledge: Knowledge::Revealed(s7),
            supersedes: None,
        })
        .await
        .expect("create");

    assert_eq!(session_origin_ordinal(&view.origin), 3);
    let KnowledgeView::Revealed(reveal) = view.knowledge else {
        panic!("expected Revealed knowledge");
    };
    assert_eq!(reveal.ordinal, 7, "reveal ordinal kept from validation");
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
async fn hidden_then_reveal_then_conceal() {
    // The knowledge axis lifecycle on a secret fact: Hidden -> Revealed(s) -> Hidden,
    // all reversible. Each PATCH sets the knowledge wholesale.
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s11 = seed_session(&conn, 11).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "owes a debt to".into(),
            predicate_reverse: "holds marker on".into(),
            origin: Origin::Prior,
            knowledge: Knowledge::Hidden,
            supersedes: None,
        })
        .await
        .expect("create secret");
    assert!(matches!(view.knowledge, KnowledgeView::Hidden));

    // Reveal at S11.
    graph
        .ask(patch_knowledge(&view.id, Knowledge::Revealed(s11)))
        .await
        .expect("reveal");
    let revealed = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    match &revealed[0].knowledge {
        KnowledgeView::Revealed(o) => assert_eq!(o.ordinal, 11),
        other => panic!("expected Revealed, got {other:?}"),
    }

    // Conceal (un-reveal): back to Hidden.
    graph
        .ask(patch_knowledge(&view.id, Knowledge::Hidden))
        .await
        .expect("conceal");
    let concealed = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert!(matches!(concealed[0].knowledge, KnowledgeView::Hidden));
}

#[tokio::test]
async fn public_fact_is_freely_mutable_through_every_knowledge_state() {
    // The reversal proof: a born-public fact can be concealed and reclassified. The
    // secret bit is no longer immutable - PATCH knowledge moves it Public -> Hidden ->
    // Revealed(s) -> Public, and the view reflects each at-rest state.
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s5 = seed_session(&conn, 5).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(create(&a, &b, "is allied with", "is allied with"))
        .await
        .expect("create public");
    assert!(matches!(view.knowledge, KnowledgeView::Public));

    // Conceal: Public -> Hidden (the transition the old immutable model forbade).
    graph
        .ask(patch_knowledge(&view.id, Knowledge::Hidden))
        .await
        .expect("conceal a public fact");
    let after_conceal = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert!(matches!(after_conceal[0].knowledge, KnowledgeView::Hidden));

    // Reclassify as revealed at S5 (Prior origin imposes no lower bound).
    graph
        .ask(patch_knowledge(&view.id, Knowledge::Revealed(s5)))
        .await
        .expect("reveal");
    let after_reveal = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    match &after_reveal[0].knowledge {
        KnowledgeView::Revealed(o) => assert_eq!(o.ordinal, 5),
        other => panic!("expected Revealed, got {other:?}"),
    }

    // Back to Public: drops the reveal and the secret bit together.
    graph
        .ask(patch_knowledge(&view.id, Knowledge::Public))
        .await
        .expect("re-publicize");
    let after_public = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert!(matches!(after_public[0].knowledge, KnowledgeView::Public));
}

#[tokio::test]
async fn end_then_un_end_restores_live() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s12 = seed_session(&conn, 12).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let view = graph
        .ask(create(&a, &b, "is captain of", "is captained by"))
        .await
        .expect("create");

    // End at S12: still shown in the GM curation view, now superseded.
    graph
        .ask(patch_superseded(&view.id, StampChange::Set(s12)))
        .await
        .expect("end");
    let ended = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert_eq!(ended.len(), 1, "still in the curation view");
    assert_eq!(ended[0].superseded.expect("superseded").ordinal, 12);

    let row = relationships::Entity::find_by_id(RelationshipIdCol::from(view.id.clone()))
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(row.superseded_session_id.is_some(), "DB row is superseded");

    // Un-end (clear superseded): the reversible correction, back to live.
    graph
        .ask(patch_superseded(&view.id, StampChange::Clear))
        .await
        .expect("un-end");
    let live = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert!(live[0].superseded.is_none(), "un-ended: live again");
}

/// The three-legged reversibility proof: un-ending a row whose canonical fact is
/// already live again re-adds it to the live set and trips the partial unique index
/// as `DuplicateLiveFact` (the guard relaxation + the clearing-write error mapping +
/// the index, working together). Set up by hand (a same-pair supersede can't be
/// minted through the create path - create-first would collide on the insert).
#[tokio::test]
async fn un_end_with_a_live_successor_is_rejected() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s12 = seed_session(&conn, 12).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let r1 = graph
        .ask(create(&a, &b, "rules", "is ruled by"))
        .await
        .expect("R1");
    // End R1, freeing the canonical key.
    graph
        .ask(patch_superseded(&r1.id, StampChange::Set(s12)))
        .await
        .expect("end R1");
    // A fresh live row reclaims the same canonical key.
    graph
        .ask(create(&a, &b, "rules", "is ruled by"))
        .await
        .expect("R2 (same pair) coexists with ended R1");

    // Un-ending R1 would put two live rows on one canonical fact: rejected.
    let res = graph
        .ask(patch_superseded(&r1.id, StampChange::Clear))
        .await;
    assert!(matches!(
        res,
        Err(SendError::HandlerError(
            PatchRelationshipError::DuplicateLiveFact
        ))
    ));
    // The rejected patch rolled back: R1 stays ended.
    let r1_row = relationships::Entity::find_by_id(RelationshipIdCol::from(r1.id))
        .one(&conn)
        .await
        .unwrap()
        .unwrap();
    assert!(
        r1_row.superseded_session_id.is_some(),
        "R1 remains superseded after the rejected un-end"
    );
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
            origin: Origin::Session(s6),
            knowledge: Knowledge::Public,
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
            origin: Origin::Session(s12),
            knowledge: Knowledge::Public,
            supersedes: Some(original.id.clone()),
        })
        .await
        .expect("supersede");
    assert_eq!(replacement.predicate, "is quartermaster of");
    assert!(
        replacement.superseded.is_none(),
        "the reply is the live new row"
    );
    assert_eq!(session_origin_ordinal(&replacement.origin), 12);

    let from_a = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert_eq!(from_a.len(), 2, "old (superseded) + new (live)");

    let old = from_a
        .iter()
        .find(|v| v.id == original.id)
        .expect("old present");
    assert_eq!(old.predicate, "is captain of", "old predicate is immutable");
    assert_eq!(old.superseded.expect("old ended").ordinal, 12);

    let new = from_a
        .iter()
        .find(|v| v.id != original.id)
        .expect("new present");
    assert_eq!(new.predicate, "is quartermaster of");
    assert!(new.superseded.is_none(), "new row is live");
    assert_eq!(
        session_origin_ordinal(&new.origin),
        12,
        "new fact originates at the supersede session"
    );
}

/// Supersede is an atomic Create+SetSuperseded batch: the "wait, they're already
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
            origin: Origin::Session(s12),
            knowledge: Knowledge::Public,
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
        r2_row.superseded_session_id.is_none(),
        "R2 was never ended; it stays live"
    );
}

#[tokio::test]
async fn retcon_then_un_retcon() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s2 = seed_session(&conn, 2).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    let v = graph
        .ask(create(&a, &b, "is brother to", "is brother to"))
        .await
        .expect("create");

    // Retcon as of S2: struck from snapshots, still in the curation view.
    graph
        .ask(patch_retcon(&v.id, StampChange::Set(s2)))
        .await
        .expect("retcon");
    let retconned = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert_eq!(retconned[0].retcon.expect("retconned").ordinal, 2);

    // Un-retcon (clear): restored.
    graph
        .ask(patch_retcon(&v.id, StampChange::Clear))
        .await
        .expect("un-retcon");
    let restored = graph
        .ask(RelationshipsForPage { page_id: a.clone() })
        .await
        .unwrap();
    assert!(restored[0].retcon.is_none(), "un-retconned");
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
        .ask(DeleteRelationship { rel_id: v.id })
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
async fn end_before_origin_is_rejected() {
    let conn = setup().await;
    let a = seed_page(&conn, "A").await;
    let b = seed_page(&conn, "B").await;
    let s6 = seed_session(&conn, 6).await;
    let s3 = seed_session(&conn, 3).await;
    let (_writer, graph) = spawn_graph(&conn).await;

    // Born at S6, then asked to end as of the earlier S3: a fact cannot end before it
    // began.
    let view = graph
        .ask(CreateRelationship {
            subject: a.clone(),
            other: b.clone(),
            predicate_forward: "is captain of".into(),
            predicate_reverse: "is captained by".into(),
            origin: Origin::Session(s6),
            knowledge: Knowledge::Public,
            supersedes: None,
        })
        .await
        .expect("create");

    let res = graph
        .ask(patch_superseded(&view.id, StampChange::Set(s3)))
        .await;
    assert!(matches!(
        res,
        Err(SendError::HandlerError(
            PatchRelationshipError::EventBeforeOrigin { .. }
        ))
    ));
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
