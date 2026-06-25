use std::time::Duration;

use chrono::Utc;
use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::loro::prosemirror::{
    ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
};
use familiar_systems_campaign_shared::loro::toc::TocPageKind;
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::{ActorRef, Spawn};
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sea_orm_migration::MigratorTrait;

use super::*;
use crate::actors::database_writer::DatabaseWriteActor;
use crate::actors::toc::{AddPageNode, ReadPageNodeTitle, TocActor, TocActorArgs};
use crate::db;
use crate::domain::crdt::room_actor;
use crate::domain::page::DocumentPageKind;
use crate::entities::columns::{BlockIdCol, PageIdCol, PageKindCol, SectionCol, StatusCol};
use crate::entities::{blocks, pages};
use crate::migrations::Migrator;

/// Spawn a ToC actor for tests that need a `PageActor`'s required `toc` ref.
/// Borrows its deps so the caller can still move `conn`/`db_writer`/
/// `campaign_id` into `PageActorArgs` afterward.
fn spawn_toc(
    campaign_id: &CampaignId,
    conn: &DatabaseConnection,
    db_writer: &ActorRef<DatabaseWriteActor>,
) -> ActorRef<TocActor> {
    TocActor::spawn(TocActorArgs {
        campaign_id: campaign_id.clone(),
        db_reader: conn.clone(),
        db_writer: db_writer.clone(),
        debounce_duration: Duration::from_secs(60),
    })
}

async fn setup_db() -> DatabaseConnection {
    db::register_sqlite_vec();
    let conn = db::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite");
    Migrator::up(&conn, None).await.expect("migrate");
    conn
}

fn insert_page(page_id: &PageId, name: &str) -> pages::ActiveModel {
    let now = Utc::now();
    pages::ActiveModel {
        id: Set(PageIdCol::from(page_id.clone())),
        name: Set(name.to_string()),
        status: Set(StatusCol::from(Status::GmOnly)),
        kind: Set(PageKindCol::from(PageKind::Entity)),
        template_id: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    }
}

fn make_heading_blob(text: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        NODE_NAME_KEY: "heading",
        ATTRIBUTES_KEY: { "level": 1 },
        CHILDREN_KEY: [text]
    }))
    .unwrap()
}

#[tokio::test]
async fn starts_with_no_blocks() {
    let conn = setup_db().await;
    let page_id = PageId::generate();
    insert_page(&page_id, "Empty Page")
        .insert(&conn)
        .await
        .unwrap();

    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

    let toc = spawn_toc(&campaign_id, &conn, &db_writer);
    let actor = PageActor::spawn(PageActorArgs {
        campaign_id,
        page_id: page_id.clone(),
        db_reader: conn,
        db_writer,
        toc,
        init: PageInit::Restore,
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(60),
    });
    actor.wait_for_startup().await;
    actor.stop_gracefully().await.unwrap();
    actor.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn new_init_persists_genesis_row() {
    let conn = setup_db().await;
    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

    let page_id = PageId::generate();
    let toc = spawn_toc(&campaign_id, &conn, &db_writer);
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let actor = PageActor::spawn(PageActorArgs {
        campaign_id,
        page_id: page_id.clone(),
        db_reader: conn.clone(),
        db_writer,
        toc,
        init: PageInit::NewDocumentPage {
            name: "Korgath the Destroyer".into(),
            kind: DocumentPageKind::Entity,
            status: Status::GmOnly,
            reply: reply_tx,
        },
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(60),
    });
    actor.wait_for_startup().await;
    assert!(actor.is_alive(), "genesis should succeed");

    // Genesis threads the committed row back, rather than the supervisor
    // re-reading it.
    let threaded = reply_rx.await.expect("genesis reply received");

    // The actor wrote its own birth row.
    let row = pages::Entity::find_by_id(PageIdCol::from(page_id.clone()))
        .one(&conn)
        .await
        .unwrap()
        .expect("genesis page row exists");
    assert_eq!(row.name, "Korgath the Destroyer");
    assert_eq!(Status::from(row.status), Status::GmOnly);
    assert_eq!(threaded, row, "threaded reply matches the persisted row");

    actor.stop_gracefully().await.unwrap();
    actor.wait_for_shutdown_with_result(|_| ()).await;
}

/// Template genesis (`PageInit::NewTemplate`) persists a `kind = template`
/// page with the same `preamble` + `body` section seeds as an entity - the
/// layout an entity later cloned from it inherits - and no `template_id`
/// lineage of its own.
#[tokio::test]
async fn new_template_init_persists_template_kind() {
    let conn = setup_db().await;
    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

    let page_id = PageId::generate();
    let toc = spawn_toc(&campaign_id, &conn, &db_writer);
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let actor = PageActor::spawn(PageActorArgs {
        campaign_id,
        page_id: page_id.clone(),
        db_reader: conn.clone(),
        db_writer,
        toc,
        init: PageInit::NewDocumentPage {
            name: "NPC".into(),
            kind: DocumentPageKind::Template,
            status: Status::GmOnly,
            reply: reply_tx,
        },
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(60),
    });
    actor.wait_for_startup().await;
    assert!(actor.is_alive(), "template genesis should succeed");

    let threaded = reply_rx.await.expect("genesis reply received");

    let row = pages::Entity::find_by_id(PageIdCol::from(page_id.clone()))
        .one(&conn)
        .await
        .unwrap()
        .expect("genesis template row exists");
    assert_eq!(row.name, "NPC");
    assert_eq!(PageKind::from(row.kind), PageKind::Template);
    assert_eq!(threaded, row, "threaded reply matches the persisted row");
    assert!(
        row.template_id.is_none(),
        "a template is the source, not a clone, so it has no lineage"
    );

    // One seeded paragraph per declared section (preamble + body), identical
    // to an entity - that shared layout is the whole point of cloning.
    let block_rows = blocks::Entity::find()
        .filter(blocks::Column::PageId.eq(PageIdCol::from(page_id.clone())))
        .all(&conn)
        .await
        .unwrap();
    assert_eq!(
        block_rows.len(),
        PageKind::Template.sections().len(),
        "one seeded block per declared template section",
    );

    actor.stop_gracefully().await.unwrap();
    actor.wait_for_shutdown_with_result(|_| ()).await;
}

/// Session genesis (`PageInit::NewSession`) persists, in one transaction, a
/// Session page, its four seeded section blocks, and the linked temporal
/// `sessions` row at ordinal 1 carrying the GM's subtitle.
#[tokio::test]
async fn new_session_init_persists_page_and_linked_session() {
    use crate::entities::sessions;

    let conn = setup_db().await;
    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

    let page_id = PageId::generate();
    let toc = spawn_toc(&campaign_id, &conn, &db_writer);
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let actor = PageActor::spawn(PageActorArgs {
        campaign_id,
        page_id: page_id.clone(),
        db_reader: conn.clone(),
        db_writer,
        toc,
        init: PageInit::NewSession {
            name: "The Goblin Ambush".into(),
            status: Status::GmOnly,
            reply: reply_tx,
        },
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(60),
    });
    actor.wait_for_startup().await;
    assert!(actor.is_alive(), "session genesis should succeed");

    // Genesis threads both committed rows back, rather than the supervisor
    // re-reading them.
    let threaded = reply_rx.await.expect("genesis reply received");

    let page = pages::Entity::find_by_id(PageIdCol::from(page_id.clone()))
        .one(&conn)
        .await
        .unwrap()
        .expect("page row exists");
    assert_eq!(PageKind::from(page.kind), PageKind::Session);
    assert_eq!(
        page.name, "The Goblin Ambush",
        "the label lives on the page"
    );
    assert_eq!(
        threaded.page, page,
        "threaded page matches the persisted row"
    );

    // One seeded paragraph per session section (prep/summary/transcript/journal).
    let block_rows = blocks::Entity::find()
        .filter(blocks::Column::PageId.eq(PageIdCol::from(page_id.clone())))
        .all(&conn)
        .await
        .unwrap();
    assert_eq!(
        block_rows.len(),
        PageKind::Session.sections().len(),
        "one seeded block per declared session section",
    );

    let session = sessions::Entity::find()
        .filter(sessions::Column::PageId.eq(PageIdCol::from(page_id.clone())))
        .one(&conn)
        .await
        .unwrap()
        .expect("session row exists for the page");
    assert_eq!(session.ordinal, 1);
    assert_eq!(
        threaded.session, session,
        "threaded session matches the persisted row"
    );

    actor.stop_gracefully().await.unwrap();
    actor.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn restores_from_blocks() {
    let conn = setup_db().await;
    let page_id = PageId::generate();
    insert_page(&page_id, "Korgath")
        .insert(&conn)
        .await
        .unwrap();

    let now = Utc::now();
    blocks::ActiveModel {
        id: Set(BlockIdCol::from(BlockId::generate())),
        page_id: Set(PageIdCol::from(page_id.clone())),
        status: Set(StatusCol::from(Status::GmOnly)),
        ordering: Set(0),
        content: Set(make_heading_blob("Korgath the Destroyer")),
        section: Set(SectionCol::Body),
        created_at: Set(now),
        updated_at: Set(now),
    }
    .insert(&conn)
    .await
    .unwrap();

    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

    let toc = spawn_toc(&campaign_id, &conn, &db_writer);
    let actor = PageActor::spawn(PageActorArgs {
        campaign_id,
        page_id,
        db_reader: conn,
        db_writer,
        toc,
        init: PageInit::Restore,
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(60),
    });
    actor.wait_for_startup().await;
    actor.stop_gracefully().await.unwrap();
    actor.wait_for_shutdown_with_result(|_| ()).await;
}

/// A freshly spawned room is born vacating: with no client ever joining, the
/// idle timer must still fire and self-evict, rather than leaking the actor
/// resident until campaign drain.
#[tokio::test]
async fn born_vacating_self_evicts_without_a_join() {
    let conn = setup_db().await;
    let page_id = PageId::generate();
    insert_page(&page_id, "Unopened")
        .insert(&conn)
        .await
        .unwrap();

    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

    let toc = spawn_toc(&campaign_id, &conn, &db_writer);
    let actor = PageActor::spawn(PageActorArgs {
        campaign_id,
        page_id,
        db_reader: conn,
        db_writer,
        toc,
        init: PageInit::Restore,
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_millis(40),
    });
    actor.wait_for_startup().await;

    // No client joins; the born-vacating idle timer should evict it.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(!actor.is_alive(), "an un-joined page self-evicts when idle");
}

/// The server-authoritative title path, end to end: a GM joins a Page room,
/// edits `meta.title` over a `ClientUpdate`, and the owning PageActor pushes
/// the rename to the wired TocActor so the live ToC node reflects it.
#[tokio::test]
async fn meta_title_edit_pushes_rename_to_toc() {
    use familiar_systems_campaign_shared::id::ClientId;
    use familiar_systems_campaign_shared::loro::page::{CONTAINER_META, KEY_TITLE};
    use loro::LoroDoc;
    use std::borrow::Cow;

    let conn = setup_db().await;
    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });
    let toc = TocActor::spawn(TocActorArgs {
        campaign_id: campaign_id.clone(),
        db_reader: conn.clone(),
        db_writer: db_writer.clone(),
        debounce_duration: Duration::from_secs(60),
    });
    toc.wait_for_startup().await;

    // Genesis a Page named "Original" (writes its own row), wired to the toc.
    let page_id = PageId::generate();
    // This test exercises the title-rename path, not genesis; ignore the
    // threaded reply.
    let (genesis_tx, _genesis_rx) = tokio::sync::oneshot::channel();
    let page = PageActor::spawn(PageActorArgs {
        campaign_id: campaign_id.clone(),
        page_id: page_id.clone(),
        db_reader: conn.clone(),
        db_writer,
        toc: toc.clone(),
        init: PageInit::NewDocumentPage {
            name: "Original".into(),
            kind: DocumentPageKind::Entity,
            status: Status::GmOnly,
            reply: genesis_tx,
        },
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(60),
    });
    page.wait_for_startup().await;

    // Mirror the supervisor: place the page's node in the ToC.
    toc.ask(AddPageNode {
        page_id: page_id.clone(),
        title: "Original".into(),
        page_kind: TocPageKind::Entity,
        visibility: Status::GmOnly,
        parent: None,
    })
    .await
    .expect("add page node");
    assert_eq!(
        toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap(),
        Some("Original".to_string()),
    );

    // Join as GM (Write capability) and take the server snapshot.
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let client = ClientId::new(1);
    let join = page
        .ask(room_actor::ClientJoin {
            client,
            tx,
            role: CampaignRole::Gm,
        })
        .await
        .expect("join");

    // Build a client doc from the snapshot, rename via meta.title, and ship
    // the delta back as a ClientUpdate (the real edit path).
    let client_doc = LoroDoc::new();
    client_doc.import(join.snapshot.as_bytes()).unwrap();
    let before = client_doc.oplog_vv();
    client_doc
        .get_map(CONTAINER_META)
        .insert(KEY_TITLE, "Renamed")
        .unwrap();
    client_doc.commit();
    let update = client_doc
        .export(loro::ExportMode::Updates {
            from: Cow::Owned(before),
        })
        .unwrap();

    page.ask(room_actor::ClientUpdate {
        client,
        updates: vec![update],
    })
    .await
    .expect("apply update");

    // The PageActor spawns the push to the TocActor, so the rename lands
    // asynchronously after the ClientUpdate reply returns; poll until it does.
    let mut renamed = false;
    for _ in 0..50 {
        if toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap() == Some("Renamed".to_string())
        {
            renamed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        renamed,
        "PageActor must push the rename to the live ToC node"
    );

    page.stop_gracefully().await.unwrap();
    page.wait_for_shutdown_with_result(|_| ()).await;
    toc.stop_gracefully().await.unwrap();
    toc.wait_for_shutdown_with_result(|_| ()).await;
}

/// An empty `meta.title` should never happen (Pages are name-first), but if
/// a client commits one it must surface loudly, not silently keep the stale
/// title. Clearing the title pushes the deterministic recovery marker to the
/// live ToC node; the old `if let Some(title)` gate would have dropped it.
#[tokio::test]
async fn clearing_title_pushes_recovery_marker_to_toc() {
    use familiar_systems_campaign_shared::id::ClientId;
    use familiar_systems_campaign_shared::loro::page::{CONTAINER_META, KEY_TITLE};
    use loro::LoroDoc;
    use std::borrow::Cow;

    let conn = setup_db().await;
    let campaign_id = CampaignId::generate();
    let db_writer =
        DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });
    let toc = TocActor::spawn(TocActorArgs {
        campaign_id: campaign_id.clone(),
        db_reader: conn.clone(),
        db_writer: db_writer.clone(),
        debounce_duration: Duration::from_secs(60),
    });
    toc.wait_for_startup().await;

    // Genesis a Page named "Original" (writes its own row), wired to the toc.
    let page_id = PageId::generate();
    // This test exercises the title-rename path, not genesis; ignore the
    // threaded reply.
    let (genesis_tx, _genesis_rx) = tokio::sync::oneshot::channel();
    let page = PageActor::spawn(PageActorArgs {
        campaign_id: campaign_id.clone(),
        page_id: page_id.clone(),
        db_reader: conn.clone(),
        db_writer,
        toc: toc.clone(),
        init: PageInit::NewDocumentPage {
            name: "Original".into(),
            kind: DocumentPageKind::Entity,
            status: Status::GmOnly,
            reply: genesis_tx,
        },
        debounce_duration: Duration::from_secs(60),
        idle_timeout: Duration::from_secs(60),
    });
    page.wait_for_startup().await;

    // Mirror the supervisor: place the page's node in the ToC.
    toc.ask(AddPageNode {
        page_id: page_id.clone(),
        title: "Original".into(),
        page_kind: TocPageKind::Entity,
        visibility: Status::GmOnly,
        parent: None,
    })
    .await
    .expect("add page node");

    // Join as GM, then clear meta.title to "" over a ClientUpdate.
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let client = ClientId::new(1);
    let join = page
        .ask(room_actor::ClientJoin {
            client,
            tx,
            role: CampaignRole::Gm,
        })
        .await
        .expect("join");

    let client_doc = LoroDoc::new();
    client_doc.import(join.snapshot.as_bytes()).unwrap();
    let before = client_doc.oplog_vv();
    client_doc
        .get_map(CONTAINER_META)
        .insert(KEY_TITLE, "")
        .unwrap();
    client_doc.commit();
    let update = client_doc
        .export(loro::ExportMode::Updates {
            from: Cow::Owned(before),
        })
        .unwrap();

    page.ask(room_actor::ClientUpdate {
        client,
        updates: vec![update],
    })
    .await
    .expect("apply update");

    // The push lands asynchronously; poll until the ToC node carries the
    // recovery marker (the page's id makes it identifiable).
    let marker = format!("ERROR LOADING TITLE {}", page_id.0);
    let mut surfaced = false;
    for _ in 0..50 {
        if toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap() == Some(marker.clone()) {
            surfaced = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        surfaced,
        "clearing the title must push the recovery marker to the live ToC node"
    );

    page.stop_gracefully().await.unwrap();
    page.wait_for_shutdown_with_result(|_| ()).await;
    toc.stop_gracefully().await.unwrap();
    toc.wait_for_shutdown_with_result(|_| ()).await;
}
