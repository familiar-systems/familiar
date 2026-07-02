use std::time::Duration;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::loro::toc::TocPageKind;
use familiar_systems_campaign_shared::status::Status;
use kameo::message::{Context, Message};
use sea_orm::ActiveValue::Set;

use super::*;
use crate::domain::crdt::doc::VersionVector;
use crate::entities::columns::{PageIdCol, PageKindCol, StatusCol};
use crate::entities::{pages, toc_entries};

/// Test-only probe: ask the actor whether it currently holds unpersisted
/// edits, so a test can assert dirtiness without reaching into private state.
#[derive(Debug, Clone, Copy)]
struct InspectDirty;

impl Message<InspectDirty> for TocActor {
    type Reply = bool;
    async fn handle(
        &mut self,
        _: InspectDirty,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.persist.is_dirty()
    }
}

// -- Actor: AddPageNode --

/// Proves the critical `known_pages` wiring: a node added via
/// `AddPageNode` survives `snapshot_toc` (which drops Page entries not in
/// `known_pages`) and is persisted. Forced through `on_stop` rather than
/// the debounce timer for determinism.
#[tokio::test]
async fn add_page_node_tracks_known_page_and_persists() {
    use crate::actors::database_writer::{DatabaseWriteActor, DatabaseWriteActorArgs, Ping};
    use crate::db;
    use crate::migrations::Migrator;
    use chrono::Utc;
    use kameo::actor::Spawn;
    use sea_orm::{ActiveModelTrait, EntityTrait};
    use sea_orm_migration::MigratorTrait;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");

    let campaign_id = CampaignId::generate();
    let db_writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: campaign_id.clone(),
        conn: conn.clone(),
    });

    let toc = TocActor::spawn(TocActorArgs {
        campaign_id,
        db_reader: conn.clone(),
        db_writer: db_writer.clone(),
        debounce_duration: Duration::from_secs(60), // don't fire mid-test
    });
    toc.wait_for_startup().await;

    // Insert the backing Page row so the toc_entries FK is satisfied. The
    // running TocActor doesn't know about it yet (startup already read the
    // pages table), which is exactly the create-time situation.
    let page_id = PageId::generate();
    let now = Utc::now();
    pages::ActiveModel {
        id: Set(PageIdCol::from(page_id.clone())),
        name: Set("Korgath".into()),
        status: Set(StatusCol::from(Status::GmOnly)),
        kind: Set(PageKindCol::Entity),
        template_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&conn)
    .await
    .unwrap();

    toc.ask(AddPageNode {
        page_id: page_id.clone(),
        title: "Korgath".into(),
        page_kind: TocPageKind::Entity,
        visibility: Status::GmOnly,
        parent: None,
    })
    .await
    .expect("add page node");

    // Stopping flushes the dirty doc through on_stop -> WriteTocSnapshot.
    toc.stop_gracefully().await.unwrap();
    toc.wait_for_shutdown_with_result(|_| ()).await;
    // FIFO mailbox: Ping returns only after the snapshot write is processed.
    db_writer.ask(Ping).await.unwrap();

    let rows = toc_entries::Entity::find().all(&conn).await.unwrap();
    assert_eq!(rows.len(), 1, "the new page's toc entry was persisted");
    assert_eq!(
        rows[0].page_id.clone().map(PageId::from),
        Some(page_id),
        "persisted entry points at the created page"
    );
}

/// Bug 3 regression: a flush that fails (here, the writer is dead so every
/// `ask` errors) must leave the actor dirty, not falsely clean. The old
/// fire-and-forget path cleared `dirty` on enqueue regardless of the write's
/// fate; the `ask`-and-`after_flush` path keeps it dirty so the on-stop flush
/// and subsequent retries still fire.
#[tokio::test]
async fn failed_flush_leaves_actor_dirty() {
    use crate::actors::database_writer::{DatabaseWriteActor, DatabaseWriteActorArgs};
    use crate::db;
    use crate::migrations::Migrator;
    use kameo::actor::Spawn;
    use sea_orm_migration::MigratorTrait;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");

    let campaign_id = CampaignId::generate();
    let db_writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: campaign_id.clone(),
        conn: conn.clone(),
    });

    // Kill the writer up front so every flush `ask` fails (ActorNotRunning).
    db_writer.stop_gracefully().await.unwrap();
    db_writer.wait_for_shutdown_with_result(|_| ()).await;

    let toc = TocActor::spawn(TocActorArgs {
        campaign_id,
        db_reader: conn.clone(),
        db_writer,
        debounce_duration: Duration::from_millis(30),
    });
    toc.wait_for_startup().await;

    // A server-side ToC mutation marks dirty and schedules a doomed flush.
    // AddPageNode touches only the in-memory doc, so no backing row needed.
    toc.ask(AddPageNode {
        page_id: PageId::generate(),
        title: "Korgath".into(),
        page_kind: TocPageKind::Entity,
        visibility: Status::GmOnly,
        parent: None,
    })
    .await
    .expect("add page node");

    // Let the debounce fire and the flush fail against the dead writer.
    tokio::time::sleep(Duration::from_millis(150)).await;

    assert!(
        toc.ask(InspectDirty).await.unwrap(),
        "a failed flush must leave the actor dirty"
    );

    toc.stop_gracefully().await.unwrap();
    toc.wait_for_shutdown_with_result(|_| ()).await;
}

// -- Actor: UpdatePageNode --

/// The server-authoritative title path: a Page's owning actor pushes
/// node-state changes here. A title-only change refreshes the live tree and
/// broadcasts but schedules no snapshot (titles re-derive from `pages.name`);
/// a visibility change additionally marks the actor dirty.
#[tokio::test]
async fn update_page_node_refreshes_title_and_persists_only_on_visibility() {
    use crate::actors::database_writer::{DatabaseWriteActor, DatabaseWriteActorArgs};
    use crate::db;
    use crate::migrations::Migrator;
    use chrono::Utc;
    use kameo::actor::Spawn;
    use sea_orm::ActiveModelTrait;
    use sea_orm_migration::MigratorTrait;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");

    // A Page plus a toc_entry referencing it: restore yields a clean tree
    // (no orphan), so persist-scheduling can be asserted precisely.
    let page_id = PageId::generate();
    let now = Utc::now();
    pages::ActiveModel {
        id: Set(PageIdCol::from(page_id.clone())),
        name: Set("Original".into()),
        status: Set(StatusCol::from(Status::GmOnly)),
        kind: Set(PageKindCol::Entity),
        template_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&conn)
    .await
    .unwrap();
    toc_entries::ActiveModel {
        id: Set(ulid::Ulid::new().to_string()),
        page_id: Set(Some(PageIdCol::from(page_id.clone()))),
        folder_title: Set(None),
        visibility: Set(StatusCol::from(Status::GmOnly)),
        parent_id: Set(None),
        position: Set(0),
    }
    .insert(&conn)
    .await
    .unwrap();

    let campaign_id = CampaignId::generate();
    let db_writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: campaign_id.clone(),
        conn: conn.clone(),
    });
    let toc = TocActor::spawn(TocActorArgs {
        campaign_id,
        db_reader: conn.clone(),
        db_writer,
        debounce_duration: Duration::from_secs(60), // don't fire mid-test
    });
    toc.wait_for_startup().await;

    assert_eq!(
        toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap(),
        Some("Original".to_string()),
        "restored node carries the page's name"
    );
    assert!(!toc.ask(InspectDirty).await.unwrap(), "clean after restore");

    // Title-only change: live tree updates, nothing scheduled to persist.
    toc.ask(UpdatePageNode {
        page_id: page_id.clone(),
        title: "Renamed".into(),
        visibility: Status::GmOnly,
        // Version-gating is not yet wired (see handler TODO); values are inert.
        created_at: Utc::now(),
        version: VersionVector(Vec::new()),
    })
    .await
    .expect("update page node");
    assert_eq!(
        toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap(),
        Some("Renamed".to_string()),
        "live title updated"
    );
    assert!(
        !toc.ask(InspectDirty).await.unwrap(),
        "a title-only change must not schedule a snapshot"
    );

    // Visibility change: a snapshot is now warranted.
    toc.ask(UpdatePageNode {
        page_id: page_id.clone(),
        title: "Renamed".into(),
        visibility: Status::Known,
        created_at: Utc::now(),
        version: VersionVector(Vec::new()),
    })
    .await
    .expect("update page node visibility");
    assert!(
        toc.ask(InspectDirty).await.unwrap(),
        "a visibility change must schedule a snapshot"
    );

    toc.stop_gracefully().await.unwrap();
    toc.wait_for_shutdown_with_result(|_| ()).await;
}

// -- Actor: SeedTocFolder --

/// Template-bundle seeding: one folder at the root with the bundle's template
/// pages nested under it, persisted in a single snapshot. Proves the folder
/// gets a stable row id, each child is tracked in `known_pages` (so it survives
/// `snapshot_toc`), and each child's `parent_id` points at the folder.
#[tokio::test]
async fn seed_toc_folder_nests_pages_under_a_persisted_folder() {
    use crate::actors::database_writer::{DatabaseWriteActor, DatabaseWriteActorArgs, Ping};
    use crate::db;
    use crate::migrations::Migrator;
    use chrono::Utc;
    use kameo::actor::Spawn;
    use sea_orm::{ActiveModelTrait, EntityTrait};
    use sea_orm_migration::MigratorTrait;

    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:").await.expect("sqlite");
    Migrator::up(&conn, None).await.expect("migrate");

    let campaign_id = CampaignId::generate();
    let db_writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
        campaign_id: campaign_id.clone(),
        conn: conn.clone(),
    });
    let toc = TocActor::spawn(TocActorArgs {
        campaign_id,
        db_reader: conn.clone(),
        db_writer: db_writer.clone(),
        debounce_duration: Duration::from_secs(60), // don't fire mid-test
    });
    toc.wait_for_startup().await;

    // Backing template pages so the toc_entries FK resolves, mirroring the
    // create-time situation (the running actor didn't see them at startup).
    let now = Utc::now();
    let mut children = Vec::new();
    for name in ["NPC", "Player Character"] {
        let page_id = PageId::generate();
        pages::ActiveModel {
            id: Set(PageIdCol::from(page_id.clone())),
            name: Set(name.into()),
            status: Set(StatusCol::from(Status::GmOnly)),
            kind: Set(PageKindCol::Template),
            template_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .unwrap();
        children.push(SeedTocChild {
            page_id,
            title: name.into(),
            page_kind: TocPageKind::Template,
            visibility: Status::GmOnly,
        });
    }
    let child_ids: Vec<PageId> = children.iter().map(|c| c.page_id.clone()).collect();

    toc.ask(SeedTocFolder {
        folder_title: "Templates".into(),
        folder_visibility: Status::GmOnly,
        children,
    })
    .await
    .expect("seed toc folder");

    toc.stop_gracefully().await.unwrap();
    toc.wait_for_shutdown_with_result(|_| ()).await;
    // FIFO mailbox: Ping returns only after the snapshot write is processed.
    db_writer.ask(Ping).await.unwrap();

    let rows = toc_entries::Entity::find().all(&conn).await.unwrap();
    let folder = rows
        .iter()
        .find(|r| r.folder_title.as_deref() == Some("Templates"))
        .expect("folder row persisted");
    assert!(folder.page_id.is_none(), "a folder backs no page");
    assert!(folder.parent_id.is_none(), "folder sits at the root");

    let page_rows: Vec<_> = rows.iter().filter(|r| r.page_id.is_some()).collect();
    assert_eq!(page_rows.len(), 2, "both template pages persisted");
    for r in &page_rows {
        assert_eq!(
            r.parent_id.as_ref(),
            Some(&folder.id),
            "each template page nests under the folder"
        );
    }
    let persisted: std::collections::HashSet<PageId> = page_rows
        .iter()
        .filter_map(|r| r.page_id.clone().map(PageId::from))
        .collect();
    for id in child_ids {
        assert!(
            persisted.contains(&id),
            "child {id:?} persisted under the folder"
        );
    }
}
