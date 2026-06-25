use chrono::Utc;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::{PageId, RelationshipId};
use kameo::actor::Spawn;
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sea_orm_migration::MigratorTrait;

use super::*;
use crate::db;
use crate::domain::relationship::NewRelationship;
use crate::entities::columns::{BlockIdCol, PageIdCol, PageKindCol, SectionCol, StatusCol};
use crate::entities::{blocks, campaign_metadata, pages, relationships, sessions, toc_entries};
use crate::migrations::Migrator;

async fn spawn_with_migrations() -> (kameo::actor::ActorRef<DatabaseWriteActor>, CampaignId) {
    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite open");
    Migrator::up(&conn, None).await.expect("migrate");

    let campaign_id = CampaignId::generate();

    let now = Utc::now();
    campaign_metadata::ActiveModel {
        id: Set(campaign_metadata::METADATA_ROW_ID),
        campaign_id: Set(campaign_id.clone().into()),
        owner_user_id: Set(String::new()),
        name: Set("Untitled".into()),
        tagline: Set(None),
        game_system: Set(None),
        content_locale: Set(None),
        home_page_id: Set(None),
        wizard_completed_at: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&conn)
    .await
    .expect("seed metadata");

    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: campaign_id.clone(),
        conn,
    });
    (actor, campaign_id)
}

#[tokio::test]
async fn ping_returns_pong() {
    let (actor, _) = spawn_with_migrations().await;
    let reply = actor.ask(Ping).await.expect("ask Ping");
    assert_eq!(reply, Pong);
}

#[tokio::test]
async fn patch_with_wizard_complete_writes_metadata() {
    let (actor, _) = spawn_with_migrations().await;
    let result = actor
        .ask(PatchCampaignMetadata {
            name: Some("Curse of Strahd".into()),
            tagline: Some("Gothic horror in Barovia".into()),
            game_system: Some("D&D 5e".into()),
            content_locale: Some("en".into()),
            complete_wizard: true,
        })
        .await
        .expect("patch should succeed");

    assert!(result.wizard_just_completed);
    assert!(result.model.wizard_completed_at.is_some());
    assert_eq!(result.model.name, "Curse of Strahd");
    assert_eq!(
        result.model.tagline.as_deref(),
        Some("Gothic horror in Barovia")
    );
    assert_eq!(result.model.game_system.as_deref(), Some("D&D 5e"));
    assert_eq!(result.model.content_locale.as_deref(), Some("en"));
}

#[tokio::test]
async fn patch_without_wizard_complete_updates_fields_only() {
    let (actor, _) = spawn_with_migrations().await;
    let result = actor
        .ask(PatchCampaignMetadata {
            name: Some("Renamed Campaign".into()),
            tagline: None,
            game_system: None,
            content_locale: None,
            complete_wizard: false,
        })
        .await
        .expect("patch should succeed");

    assert!(!result.wizard_just_completed);
    assert!(result.model.wizard_completed_at.is_none());
    assert_eq!(result.model.name, "Renamed Campaign");
}

#[tokio::test]
async fn double_wizard_complete_returns_already_initialized() {
    let (actor, _) = spawn_with_migrations().await;
    actor
        .ask(PatchCampaignMetadata {
            name: Some("First".into()),
            tagline: None,
            game_system: Some("PF2e".into()),
            content_locale: Some("en".into()),
            complete_wizard: true,
        })
        .await
        .expect("first patch");

    let err = actor
        .ask(PatchCampaignMetadata {
            name: Some("Second".into()),
            tagline: None,
            game_system: Some("Blades".into()),
            content_locale: Some("en".into()),
            complete_wizard: true,
        })
        .await
        .expect_err("second wizard_complete should be rejected");

    assert!(
        matches!(
            err,
            kameo::error::SendError::HandlerError(PatchCampaignError::AlreadyInitialized)
        ),
        "expected AlreadyInitialized, got {err:?}"
    );
}

#[tokio::test]
async fn get_metadata_returns_row() {
    let (actor, _) = spawn_with_migrations().await;
    let meta = actor.ask(GetMetadata).await.expect("metadata should exist");
    assert_eq!(meta.id, campaign_metadata::METADATA_ROW_ID);
    assert_eq!(meta.name, "Untitled");
}

#[tokio::test]
async fn graceful_stop_completes() {
    let (actor, _) = spawn_with_migrations().await;
    actor.stop_gracefully().await.expect("stop_gracefully");
    actor.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn create_page_inserts_row_and_blocks_atomically() {
    use crate::domain::page::{NewBlock, NewPage};
    use familiar_systems_campaign_shared::id::{BlockId, PageId};
    use familiar_systems_campaign_shared::loro::page::Section;
    use familiar_systems_campaign_shared::page_kind::PageKind;
    use familiar_systems_campaign_shared::status::Status;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    let page_id = PageId::generate();
    let model = actor
        .ask(DbCreatePage {
            new_page: NewPage {
                id: page_id.clone(),
                name: "Korgath".into(),
                status: Status::GmOnly,
                kind: PageKind::Entity,
                template_id: None,
                blocks: vec![NewBlock {
                    id: BlockId::generate(),
                    section: Section::Body,
                    ordering: 0,
                    content: b"hello".to_vec(),
                    status: Status::GmOnly,
                }],
            },
        })
        .await
        .expect("create page");
    assert_eq!(model.name, "Korgath");

    let pages = pages::Entity::find().all(&conn).await.unwrap();
    assert_eq!(pages.len(), 1, "page row inserted");
    let block_rows = blocks::Entity::find().all(&conn).await.unwrap();
    assert_eq!(block_rows.len(), 1, "block row inserted");
    assert_eq!(block_rows[0].content, b"hello");
}

/// `DbCreateSession` mints the temporal `sessions` row atomically with the
/// page + blocks, links it via `page_id`, and assigns `ordinal = max + 1`.
/// The session's label is the page title; the temporal row stores no name.
#[tokio::test]
async fn db_create_session_mints_linked_temporal_rows_with_sequential_ordinals() {
    use crate::domain::page::{NewBlock, NewPage};
    use familiar_systems_campaign_shared::id::{BlockId, PageId};
    use familiar_systems_campaign_shared::loro::page::Section;
    use familiar_systems_campaign_shared::page_kind::PageKind;
    use familiar_systems_campaign_shared::status::Status;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    let session_page = |name: &str| NewPage {
        id: PageId::generate(),
        name: name.into(),
        status: Status::GmOnly,
        kind: PageKind::Session,
        template_id: None,
        blocks: vec![NewBlock {
            id: BlockId::generate(),
            section: Section::Prep,
            ordering: 0,
            content: b"prep".to_vec(),
            status: Status::GmOnly,
        }],
    };

    let first = actor
        .ask(DbCreateSession {
            new_page: session_page("The Heist"),
        })
        .await
        .expect("create session");
    assert_eq!(first.session.ordinal, 1);
    assert_eq!(PageKind::from(first.page.kind), PageKind::Session);
    assert_eq!(first.page.name, "The Heist", "the label lives on the page");

    let second = actor
        .ask(DbCreateSession {
            new_page: session_page("Untitled Session"),
        })
        .await
        .expect("create second session");
    assert_eq!(second.session.ordinal, 2, "max + 1");

    let s1 = sessions::Entity::find()
        .filter(sessions::Column::PageId.eq(first.page.id.clone()))
        .one(&conn)
        .await
        .unwrap()
        .expect("session row exists for the page");
    assert_eq!(s1.ordinal, 1);
    assert_eq!(s1.page_id, Some(first.page.id.clone()));

    assert_eq!(blocks::Entity::find().all(&conn).await.unwrap().len(), 2);
}

/// A `DbCreateSession` that fails mid-genesis (here a duplicate block id trips
/// the blocks insert, after the page row is already in the txn) rolls the
/// whole thing back: no orphan page, no orphan temporal row. Page + blocks +
/// session are atomic.
#[tokio::test]
async fn db_create_session_rolls_back_atomically_on_failure() {
    use crate::domain::page::{NewBlock, NewPage};
    use familiar_systems_campaign_shared::id::{BlockId, PageId};
    use familiar_systems_campaign_shared::loro::page::Section;
    use familiar_systems_campaign_shared::page_kind::PageKind;
    use familiar_systems_campaign_shared::status::Status;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    // Two blocks share one id: the blocks insert trips the PK constraint
    // after the page row has been inserted in the same txn.
    let dup = BlockId::generate();
    let block = |section| NewBlock {
        id: dup.clone(),
        section,
        ordering: 0,
        content: b"x".to_vec(),
        status: Status::GmOnly,
    };
    let doomed = NewPage {
        id: PageId::generate(),
        name: "Untitled Session".into(),
        status: Status::GmOnly,
        kind: PageKind::Session,
        template_id: None,
        blocks: vec![block(Section::Prep), block(Section::Summary)],
    };

    actor
        .ask(DbCreateSession { new_page: doomed })
        .await
        .expect_err("a duplicate block id must fail the genesis");

    assert_eq!(
        pages::Entity::find().all(&conn).await.unwrap().len(),
        0,
        "page must roll back"
    );
    assert_eq!(
        blocks::Entity::find().all(&conn).await.unwrap().len(),
        0,
        "blocks must roll back"
    );
    assert_eq!(
        sessions::Entity::find().all(&conn).await.unwrap().len(),
        0,
        "no orphan temporal row"
    );
}

/// A `WritePageBlocks` whose upsert fails must not destroy the blocks it
/// was meant to replace. The handler prunes the Page's stale blocks before
/// upserting the new set; without a transaction a failed upsert leaves the
/// blocks pruned but unreplaced (the catastrophic case, since this runs on
/// flush-on-stop while the in-memory Loro doc is being torn down).
#[tokio::test]
async fn write_page_blocks_failed_insert_preserves_existing_rows() {
    use familiar_systems_campaign_shared::id::BlockId;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    let now = Utc::now();
    let page_id = PageId::generate();
    let page_id_col = PageIdCol::from(page_id.clone());

    // FK parent: blocks.page_id -> pages.id.
    pages::ActiveModel {
        id: Set(page_id_col.clone()),
        name: Set("Korgath".into()),
        status: Set(StatusCol::GmOnly),
        kind: Set(PageKindCol::Entity),
        template_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&conn)
    .await
    .expect("seed page");

    // One pre-existing block a correct (transactional) write must preserve.
    blocks::ActiveModel {
        id: Set(BlockIdCol::from(BlockId::generate())),
        page_id: Set(page_id_col.clone()),
        status: Set(StatusCol::GmOnly),
        ordering: Set(0),
        content: Set(b"original".to_vec()),
        section: Set(SectionCol::Body),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&conn)
    .await
    .expect("seed existing block");

    // A doomed snapshot: one row whose `page_id` points at a Page that
    // doesn't exist, so the upsert trips the FK (foreign_keys are ON) and
    // fails as a unit. The handler has already pruned the existing block by
    // then. (A duplicate-PK batch no longer suffices to force failure:
    // `ON CONFLICT DO UPDATE` would resolve the conflict instead of erroring.)
    let ghost = PageId::generate();
    let doomed = vec![blocks::ActiveModel {
        id: Set(BlockIdCol::from(BlockId::generate())),
        page_id: Set(PageIdCol::from(ghost)),
        status: Set(StatusCol::GmOnly),
        ordering: Set(0),
        content: Set(b"new-a".to_vec()),
        section: Set(SectionCol::Body),
        created_at: Set(now),
        updated_at: Set(now),
    }];

    actor
        .ask(WritePageBlocks {
            page_id: page_id.clone(),
            blocks: doomed,
            name_sync: None,
        })
        .await
        .expect_err("FK-violating upsert must fail");

    // The original block survives: the failed write rolled back its prune.
    let rows = blocks::Entity::find().all(&conn).await.unwrap();
    assert_eq!(rows.len(), 1, "existing block must survive a failed write");
    assert_eq!(rows[0].content, b"original");
}

/// `created_at` survives edits, and blocks dropped from a later snapshot are
/// pruned. The upsert path updates an existing row in place (omitting
/// `CreatedAt` from the conflict-update set, so the original creation time is
/// kept) while the `NOT IN` prune removes blocks absent from the new snapshot.
/// Regression test for "created_at reset to now on every flush" (#2 in the PR
/// review): an editor flushes a Page's blocks repeatedly, and `created_at`
/// must not be clobbered each time.
#[tokio::test]
async fn write_page_blocks_preserves_created_at_and_prunes_dropped() {
    use familiar_systems_campaign_shared::id::BlockId;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    let page_id = PageId::generate();
    let page_id_col = PageIdCol::from(page_id.clone());

    // Fixed whole-second timestamps so the SQLite text round-trip is exact and
    // the test is deterministic (no `Utc::now()` sub-second flake).
    let t0 = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    let t1 = chrono::DateTime::from_timestamp(1_700_000_060, 0).unwrap();

    // FK parent: blocks.page_id -> pages.id.
    pages::ActiveModel {
        id: Set(page_id_col.clone()),
        name: Set("Korgath".into()),
        status: Set(StatusCol::GmOnly),
        kind: Set(PageKindCol::Entity),
        template_id: Set(None),
        created_at: Set(t0),
        updated_at: Set(t0),
    }
    .insert(&conn)
    .await
    .expect("seed page");

    let kept = BlockId::generate();
    let dropped = BlockId::generate();
    let row =
        |id: &BlockId, ord: i64, body: &[u8], ts: chrono::DateTime<Utc>| blocks::ActiveModel {
            id: Set(BlockIdCol::from(id.clone())),
            page_id: Set(page_id_col.clone()),
            status: Set(StatusCol::GmOnly),
            ordering: Set(ord),
            content: Set(body.to_vec()),
            section: Set(SectionCol::Body),
            created_at: Set(ts),
            updated_at: Set(ts),
        };

    // First flush: two blocks, both stamped t0.
    actor
        .ask(WritePageBlocks {
            page_id: page_id.clone(),
            blocks: vec![row(&kept, 0, b"v1", t0), row(&dropped, 1, b"d1", t0)],
            name_sync: None,
        })
        .await
        .expect("first flush");

    // Second flush: `kept` is edited and carries a *later* created_at (as a
    // real flush would, stamping `now`); `dropped` is gone from the snapshot.
    actor
        .ask(WritePageBlocks {
            page_id: page_id.clone(),
            blocks: vec![row(&kept, 0, b"v2", t1)],
            name_sync: None,
        })
        .await
        .expect("second flush");

    let rows = blocks::Entity::find().all(&conn).await.unwrap();
    assert_eq!(
        rows.len(),
        1,
        "block dropped from the snapshot must be pruned"
    );
    let b = &rows[0];
    assert_eq!(b.content, b"v2", "surviving block has the latest content");
    assert_eq!(
        b.created_at, t0,
        "created_at must be preserved across the edit, not reset to the flush time"
    );
    assert_eq!(
        b.updated_at, t1,
        "updated_at must advance to the latest flush"
    );
}

/// A block that moves between a page's section editors keeps the same id, so
/// the flush upserts it (id is in the keep-set, prune spares it) rather than
/// re-inserting. `Section` must be in the conflict-update set or the row's
/// `section` would stay stale while its content/ordering reflect the new
/// section -- a drift the column lies about. Regression guard for including
/// `blocks::Column::Section` in the upsert; `created_at` is still preserved.
#[tokio::test]
async fn write_page_blocks_updates_section_when_a_block_moves() {
    use familiar_systems_campaign_shared::id::BlockId;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    let page_id = PageId::generate();
    let page_id_col = PageIdCol::from(page_id.clone());

    // Fixed whole-second timestamps so the SQLite text round-trip is exact.
    let t0 = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
    let t1 = chrono::DateTime::from_timestamp(1_700_000_060, 0).unwrap();

    // FK parent: blocks.page_id -> pages.id.
    pages::ActiveModel {
        id: Set(page_id_col.clone()),
        name: Set("Korgath".into()),
        status: Set(StatusCol::GmOnly),
        kind: Set(PageKindCol::Entity),
        template_id: Set(None),
        created_at: Set(t0),
        updated_at: Set(t0),
    }
    .insert(&conn)
    .await
    .expect("seed page");

    let block = BlockId::generate();
    let row = |section: SectionCol, ord: i64, body: &[u8], ts: chrono::DateTime<Utc>| {
        blocks::ActiveModel {
            id: Set(BlockIdCol::from(block.clone())),
            page_id: Set(page_id_col.clone()),
            status: Set(StatusCol::GmOnly),
            ordering: Set(ord),
            content: Set(body.to_vec()),
            section: Set(section),
            created_at: Set(ts),
            updated_at: Set(ts),
        }
    };

    // First flush: the block lives in `body`.
    actor
        .ask(WritePageBlocks {
            page_id: page_id.clone(),
            blocks: vec![row(SectionCol::Body, 1, b"v1", t0)],
            name_sync: None,
        })
        .await
        .expect("first flush");

    // Second flush: same id, now in `preamble` at a new ordering/content (as
    // a flush would after the block moved between section editors).
    actor
        .ask(WritePageBlocks {
            page_id: page_id.clone(),
            blocks: vec![row(SectionCol::Preamble, 0, b"v2", t1)],
            name_sync: None,
        })
        .await
        .expect("second flush");

    let rows = blocks::Entity::find().all(&conn).await.unwrap();
    assert_eq!(
        rows.len(),
        1,
        "the block was updated in place, not duplicated"
    );
    let b = &rows[0];
    assert_eq!(
        b.section,
        SectionCol::Preamble,
        "section must update in place when a block moves, not keep its old value",
    );
    assert_eq!(
        b.content, b"v2",
        "content updated to the new section's value"
    );
    assert_eq!(b.ordering, 0, "ordering updated to the new section's value");
    assert_eq!(
        b.created_at, t0,
        "created_at survives the move (omitted from the conflict-update set)",
    );
}

/// A `WriteTocSnapshot` whose insert fails must leave the prior rows intact.
/// The handler prunes stale rows before upserting; without a transaction a
/// failed upsert leaves a partially-applied snapshot (pruned but not
/// rewritten).
#[tokio::test]
async fn write_toc_snapshot_failed_insert_preserves_existing_rows() {
    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    // A stale folder row a correct snapshot prune would remove, but a
    // *failed* snapshot must leave untouched.
    toc_entries::ActiveModel {
        id: Set("old".to_string()),
        page_id: Set(None),
        folder_title: Set(Some("Stale Folder".into())),
        visibility: Set(StatusCol::GmOnly),
        parent_id: Set(None),
        position: Set(0),
    }
    .insert(&conn)
    .await
    .expect("seed stale toc row");

    // Keep-set is just "new"; its page_id points at a Page that doesn't
    // exist, so the upsert trips the FK (foreign_keys are ON) and fails.
    // The prune deletes "old" first.
    let ghost = PageId::generate();
    let rows = vec![toc_entries::ActiveModel {
        id: Set("new".to_string()),
        page_id: Set(Some(PageIdCol::from(ghost))),
        folder_title: Set(None),
        visibility: Set(StatusCol::GmOnly),
        parent_id: Set(None),
        position: Set(0),
    }];

    actor
        .ask(WriteTocSnapshot { rows })
        .await
        .expect_err("FK violation must fail the upsert");

    // The stale row survives: the failed write rolled back its prune.
    let surviving = toc_entries::Entity::find().all(&conn).await.unwrap();
    assert_eq!(surviving.len(), 1, "stale row must survive a failed write");
    assert_eq!(surviving[0].id, "old");
}

// ---- Relationship batch (`ApplyRelationshipWrites`) ----

async fn seed_rel_page(conn: &DatabaseConnection, name: &str) -> PageId {
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

fn new_rel(page_a: PageId, page_b: PageId, fwd: &str, rev: &str) -> NewRelationship {
    use crate::domain::relationship::{Knowledge, Origin};
    NewRelationship {
        id: RelationshipId::generate(),
        page_a,
        page_b,
        predicate_a_to_b: fwd.into(),
        predicate_b_to_a: rev.into(),
        origin: Origin::Prior,
        superseded: None,
        retcon: None,
        knowledge: Knowledge::Public,
    }
}

/// A batch commits all writes or none. Two valid `Create`s land together, and the
/// `Upserted` outcomes come back in write order.
#[tokio::test]
async fn apply_relationship_writes_commits_all_in_one_txn() {
    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    let a = seed_rel_page(&conn, "A").await;
    let b = seed_rel_page(&conn, "B").await;
    let c = seed_rel_page(&conn, "C").await;

    let outcomes = actor
        .ask(ApplyRelationshipWrites {
            writes: vec![
                RelationshipWrite::Create(new_rel(a.clone(), b, "rules", "is ruled by")),
                RelationshipWrite::Create(new_rel(a, c, "guards", "is guarded by")),
            ],
        })
        .await
        .expect("batch commits");

    assert_eq!(outcomes.len(), 2, "one outcome per write, in order");
    assert!(
        outcomes
            .iter()
            .all(|o| matches!(o, RelationshipWriteOutcome::Upserted(_))),
        "both creates upserted",
    );
    assert_eq!(
        relationships::Entity::find()
            .all(&conn)
            .await
            .unwrap()
            .len(),
        2,
    );
}

/// A batch is atomic: a later write's failure rolls back the earlier ones. Here a
/// valid `Create` is followed by a `SetStamp` on a nonexistent relationship
/// (`NotFound`); the create must not survive. This is the atomicity the
/// `RelationshipGraph` relies on for supersede but cannot exercise itself (it
/// pre-validates rel-ids before issuing the batch). Mirrors
/// `db_create_session_rolls_back_atomically_on_failure`.
#[tokio::test]
async fn apply_relationship_writes_rolls_back_on_failed_action() {
    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: CampaignId::generate(),
        conn: conn.clone(),
    });

    let a = seed_rel_page(&conn, "A").await;
    let b = seed_rel_page(&conn, "B").await;
    let ghost = RelationshipId::generate();

    let err = actor
        .ask(ApplyRelationshipWrites {
            writes: vec![
                RelationshipWrite::Create(new_rel(a, b, "rules", "is ruled by")),
                RelationshipWrite::SetStamp {
                    rel_id: ghost,
                    column: StampColumn::Superseded,
                    at: None,
                },
            ],
        })
        .await
        .expect_err("a NotFound on the second write must fail the batch");
    assert!(
        matches!(
            err,
            kameo::error::SendError::HandlerError(RelationshipWriteError::NotFound)
        ),
        "expected NotFound, got {err:?}",
    );

    assert_eq!(
        relationships::Entity::find()
            .all(&conn)
            .await
            .unwrap()
            .len(),
        0,
        "the create must roll back when a later write in the batch fails",
    );
}
