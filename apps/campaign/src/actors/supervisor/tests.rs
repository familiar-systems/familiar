use std::sync::Arc;
use std::time::Duration;

use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign_shared::id::{BlockId, ClientId, PageId};
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::Spawn;
use kameo::error::SendError;
use kameo::message::{Context, Message};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tempfile::TempDir;

use super::*;
use crate::db::register_sqlite_vec;
use crate::domain::crdt::room_actor;
use crate::domain::page::DocumentPageKind;
use crate::entities::columns::PageIdCol;
use crate::entities::{campaign_metadata, pages};
use crate::persistence::{CampaignStore, LocalCampaignStore};

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct GetStopCause;

#[cfg(test)]
impl Message<GetStopCause> for CampaignSupervisor {
    type Reply = Option<StopCause>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        _: GetStopCause,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.stop_cause
    }
}

/// Test-only probe for the private `pages` map: is this Page still tracked?
/// Lets eviction tests assert pruning without exposing the map.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ContainsPage(pub PageId);

#[cfg(test)]
impl Message<ContainsPage> for CampaignSupervisor {
    type Reply = bool;

    async fn handle(
        &mut self,
        ContainsPage(id): ContainsPage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.pages.contains_key(&id)
    }
}

fn ensure_vec0() {
    register_sqlite_vec();
}

fn store_in(dir: &std::path::Path) -> Arc<dyn CampaignStore> {
    Arc::new(LocalCampaignStore::new(dir.to_path_buf()))
}

fn fast_args(
    campaign_id: CampaignId,
    store: Arc<dyn CampaignStore>,
    idle_ms: u64,
    check_ms: u64,
) -> CampaignSupervisorArgs {
    CampaignSupervisorArgs {
        campaign_id,
        owner_user_id: Some(UserId::generate()),
        store,
        idle_timeout: Duration::from_millis(idle_ms),
        eviction_check_interval: Duration::from_millis(check_ms),
        platform_client: None,
    }
}

#[tokio::test]
async fn checkout_creates_db_file() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let campaign_id = CampaignId::generate();
    let args = fast_args(campaign_id.clone(), store, 60_000, 60_000);
    let actor_ref = CampaignSupervisor::spawn(args);
    actor_ref.wait_for_startup().await;

    assert!(tmp.path().join(format!("{}.db", campaign_id.0)).exists());

    actor_ref.stop_gracefully().await.unwrap();
    actor_ref.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn ping_returns_pong_and_bumps_activity() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
    let actor_ref = CampaignSupervisor::spawn(args);
    actor_ref.wait_for_startup().await;

    let reply = actor_ref.ask(Ping).await.unwrap();
    assert_eq!(reply, Pong);

    actor_ref.stop_gracefully().await.unwrap();
    actor_ref.wait_for_shutdown_with_result(|_| ()).await;
}

/// Proves the kameo `SendError` -> `UpdateError` mapping at the
/// `RoomHandle::update` seam: a stopped room actor yields `RoomGone`, not a
/// flattened error string. This is the boundary the `classify_update_error`
/// unit test cannot reach, and the exact case the old substring match
/// silently misread (kameo's Display is `"actor stopped"`, never the
/// `"ActorStopped"` Debug casing the connection layer was matching).
#[tokio::test]
async fn update_to_stopped_room_actor_is_room_gone() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
    let supervisor = CampaignSupervisor::spawn(args);
    supervisor.wait_for_startup().await;

    let handle = supervisor
        .ask(JoinRoom {
            room_id: "toc".to_string(),
        })
        .await
        .unwrap();

    // Stop the underlying ToC actor so the next send fails at the transport
    // layer (ActorStopped/ActorNotRunning) rather than in the handler.
    let RoomHandle::Toc(ref toc) = handle else {
        panic!("expected a ToC room handle");
    };
    toc.stop_gracefully().await.unwrap();
    toc.wait_for_shutdown_with_result(|_| ()).await;

    let err = handle
        .update(ClientId::new(1), vec![vec![0u8]])
        .await
        .expect_err("update to a stopped actor must fail");
    assert!(
        matches!(err, room_actor::UpdateError::RoomGone),
        "expected RoomGone, got {err:?}",
    );

    supervisor.stop_gracefully().await.unwrap();
    supervisor.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn set_stop_cause_is_first_writer_wins() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
    let actor_ref = CampaignSupervisor::spawn(args);
    actor_ref.wait_for_startup().await;

    actor_ref.tell(SetStopCause(StopCause::Idle)).await.unwrap();
    actor_ref
        .tell(SetStopCause(StopCause::Drain))
        .await
        .unwrap();
    let cause = actor_ref.ask(GetStopCause).await.unwrap();
    assert_eq!(cause, Some(StopCause::Idle));

    actor_ref.stop_gracefully().await.unwrap();
    actor_ref.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn idle_supervisor_self_evicts() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let args = fast_args(CampaignId::generate(), store, 30, 20);
    let actor_ref = CampaignSupervisor::spawn(args);
    actor_ref.wait_for_startup().await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    actor_ref.wait_for_shutdown_with_result(|_| ()).await;
    assert!(actor_ref.ask(Ping).await.is_err());
}

/// A PageActor that stops (idle self-eviction, or any stop) must be pruned
/// from the supervisor's `pages` map via the `link` + `on_link_died` edge,
/// not left as a dead `ActorRef` until the next join of the same id.
/// Drives the terminal effect directly with `stop_gracefully` (a `Normal`
/// stop, same as `IdleEvict -> ctx.stop`) so the test is deterministic and
/// needs no real idle wait.
#[tokio::test]
async fn evicted_page_actor_is_pruned_from_map() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
    let supervisor = CampaignSupervisor::spawn(args);
    supervisor.wait_for_startup().await;

    // Create a Page: it is inserted into `pages` and linked.
    let model = supervisor
        .ask(CreatePage {
            name: "Ephemeral".to_string(),
            status: Some(Status::GmOnly),
            parent: None,
            kind: DocumentPageKind::Entity,
        })
        .await
        .unwrap();
    let page_id = PageId::from(model.id.clone());
    assert!(
        supervisor.ask(ContainsPage(page_id.clone())).await.unwrap(),
        "newly created page should be tracked",
    );

    // Join to obtain the live actor ref (returns the same in-map actor),
    // then stop it.
    let handle = supervisor
        .ask(JoinRoom {
            room_id: format!("page:{}", page_id.0),
        })
        .await
        .unwrap();
    let RoomHandle::Page(actor) = handle else {
        panic!("expected a Page room handle");
    };
    actor.stop_gracefully().await.unwrap();
    actor.wait_for_shutdown_with_result(|_| ()).await;

    // link_died is delivered after the actor terminates; poll until pruned.
    let mut pruned = false;
    for _ in 0..50 {
        if !supervisor.ask(ContainsPage(page_id.clone())).await.unwrap() {
            pruned = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        pruned,
        "dead page actor should be pruned from the supervisor map"
    );

    supervisor.stop_gracefully().await.unwrap();
    supervisor.wait_for_shutdown_with_result(|_| ()).await;
}

/// Poll the campaign's SQLite file (the seed runs in a spawned task after
/// `on_start`, so it lands asynchronously) until the home base exists.
/// Asserts exactly one Page named "Campaign Base Camp" (status `Known`)
/// with `home_page_id` pointing at it, and returns its id.
async fn poll_until_seeded(db_path: &std::path::Path) -> PageId {
    for _ in 0..200 {
        let conn = crate::db::connect_readonly(db_path)
            .await
            .expect("open readonly");
        let pages = pages::Entity::find().all(&conn).await.expect("query pages");
        let meta = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&conn)
            .await
            .expect("query metadata")
            .expect("metadata row exists");
        // Require both writes (the Page row, then the pointer) so we never
        // observe the brief window between CreatePage and SetLandingPage.
        if let (Some(page), Some(home)) = (pages.first(), meta.home_page_id.clone()) {
            assert_eq!(pages.len(), 1, "exactly one Page seeded");
            assert_eq!(page.name, "Campaign Base Camp");
            assert_eq!(Status::from(page.status), Status::Known);
            let page_id = PageId::from(page.id.clone());
            assert_eq!(
                PageId::from(home),
                page_id,
                "home_page_id points at the base camp"
            );

            // The seed must give the home page one block per declared section
            // (preamble + body), each a paragraph whose row id equals the ULID
            // embedded in its content (`attributes.blockId`). This proves every
            // section opens schema-valid (>=1 block) and that block identity is
            // stable, not minted fresh on each persist.
            let block_rows = crate::entities::blocks::Entity::find()
                .filter(
                    crate::entities::blocks::Column::PageId.eq(PageIdCol::from(page_id.clone())),
                )
                .all(&conn)
                .await
                .expect("query blocks");
            assert_eq!(
                block_rows.len(),
                PageKind::Entity.sections().len(),
                "home page seeded with one block per section",
            );
            for block in &block_rows {
                let row_id = BlockId::from(block.id.clone()).to_string();
                let content: serde_json::Value =
                    serde_json::from_slice(&block.content).expect("seed block content is JSON");
                assert_eq!(
                    content["attributes"]["blockId"].as_str(),
                    Some(row_id.as_str()),
                    "block row id equals the blockId embedded in its content",
                );
                assert_eq!(content["nodeName"].as_str(), Some("paragraph"));
            }
            use familiar_systems_campaign_shared::loro::page::Section;
            let mut seeded_sections: Vec<&str> = block_rows
                .iter()
                .map(|b| Section::from(b.section).as_str())
                .collect();
            seeded_sections.sort();
            let mut expected_sections: Vec<&str> = PageKind::Entity
                .sections()
                .iter()
                .map(|s| s.as_str())
                .collect();
            expected_sections.sort();
            assert_eq!(
                seeded_sections, expected_sections,
                "one block seeded in each declared section",
            );

            return page_id;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("home base was not seeded within timeout");
}

#[tokio::test]
async fn brand_new_campaign_seeds_home_base() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let campaign_id = CampaignId::generate();
    let args = fast_args(campaign_id.clone(), store, 60_000, 60_000);
    let actor_ref = CampaignSupervisor::spawn(args);
    actor_ref.wait_for_startup().await;

    let db_path = tmp.path().join(format!("{}.db", campaign_id.0));
    poll_until_seeded(&db_path).await;

    actor_ref.stop_gracefully().await.unwrap();
    actor_ref.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn reopen_does_not_reseed_home_base() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let campaign_id = CampaignId::generate();
    let db_path = tmp.path().join(format!("{}.db", campaign_id.0));

    // First open seeds the base camp.
    let first = CampaignSupervisor::spawn(fast_args(
        campaign_id.clone(),
        store.clone(),
        60_000,
        60_000,
    ));
    first.wait_for_startup().await;
    let seeded = poll_until_seeded(&db_path).await;
    first.stop_gracefully().await.unwrap();
    first.wait_for_shutdown_with_result(|_| ()).await;

    // Reopen is a cold checkout (`is_new == false`): the existing metadata
    // row means the seed guard does not fire, so no second base camp.
    let second = CampaignSupervisor::spawn(fast_args(
        campaign_id.clone(),
        store.clone(),
        60_000,
        60_000,
    ));
    second.wait_for_startup().await;
    // Give any (erroneous) seed task a chance to run before asserting.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let conn = crate::db::connect_readonly(&db_path)
        .await
        .expect("open readonly");
    let pages = pages::Entity::find().all(&conn).await.expect("query pages");
    assert_eq!(pages.len(), 1, "reopen must not add a second base camp");
    let meta = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
        .one(&conn)
        .await
        .expect("query metadata")
        .expect("metadata row exists");
    assert_eq!(
        meta.home_page_id.map(PageId::from),
        Some(seeded),
        "home pointer unchanged on reopen"
    );

    second.stop_gracefully().await.unwrap();
    second.wait_for_shutdown_with_result(|_| ()).await;
}

/// Pages must have a non-empty title. An empty or whitespace-only name is
/// rejected before anything is persisted, on every creation path (not just
/// the UI).
#[tokio::test]
async fn create_page_rejects_empty_name() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
    let supervisor = CampaignSupervisor::spawn(args);
    supervisor.wait_for_startup().await;

    for name in ["", "   "] {
        let err = supervisor
            .ask(CreatePage {
                name: name.to_string(),
                status: Some(Status::GmOnly),
                parent: None,
                kind: DocumentPageKind::Entity,
            })
            .await
            .expect_err("empty name must be rejected");
        assert!(
            matches!(err, SendError::HandlerError(CreatePageError::EmptyName)),
            "expected EmptyName, got {err:?}"
        );
    }

    supervisor.stop_gracefully().await.unwrap();
    supervisor.wait_for_shutdown_with_result(|_| ()).await;
}

/// Sessions are named like every other page kind: an empty or whitespace-only
/// name is rejected before anything is persisted (the empty-name guard is no
/// longer entity/template-only).
#[tokio::test]
async fn create_session_rejects_empty_name() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
    let supervisor = CampaignSupervisor::spawn(args);
    supervisor.wait_for_startup().await;

    for name in ["", "   "] {
        let err = supervisor
            .ask(CreateSession {
                name: name.to_string(),
                status: Some(Status::GmOnly),
                parent: None,
            })
            .await
            .expect_err("empty session name must be rejected");
        assert!(
            matches!(err, SendError::HandlerError(CreateSessionError::EmptyName)),
            "expected EmptyName, got {err:?}"
        );
    }

    supervisor.stop_gracefully().await.unwrap();
    supervisor.wait_for_shutdown_with_result(|_| ()).await;
}

/// End-to-end template seeding through the supervisor: compile a real bundled
/// template, seed it, and assert it lands as a `template`-kind Page with its
/// per-block visibility preserved and nested under the `Templates` ToC folder.
/// Exercises the compile -> genesis -> ToC-folder wiring the route drives on
/// wizard completion.
#[tokio::test]
async fn seed_template_bundle_creates_template_pages_and_folder() {
    use crate::entities::blocks;
    use crate::entities::columns::{PageIdCol, PageKindCol};
    use crate::entities::toc_entries;
    use crate::starter_content::catalog::Catalog;
    use crate::starter_content::compile::compile_template;

    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let store = store_in(tmp.path());
    let campaign_id = CampaignId::generate();
    let db_path = tmp.path().join(format!("{}.db", campaign_id.0));
    let supervisor =
        CampaignSupervisor::spawn(fast_args(campaign_id.clone(), store, 60_000, 60_000));
    supervisor.wait_for_startup().await;

    // Compile a real bundled template exactly as the route does.
    let raw = Catalog::load_from_embedded().unwrap();
    let npc = compile_template(raw.template_markdown("common/npc", "en").unwrap()).unwrap();

    // `SeedTemplateBundle` replies `()`, so `ask` returns only after genesis and
    // ToC placement have run.
    supervisor
        .ask(SeedTemplateBundle {
            folder_title: "Templates".to_string(),
            templates: vec![npc],
        })
        .await
        .unwrap();

    // Page + blocks are persisted synchronously at genesis; read them live.
    let conn = crate::db::connect_readonly(&db_path).await.unwrap();
    let template_pages = pages::Entity::find()
        .filter(pages::Column::Kind.eq(PageKindCol::Template))
        .all(&conn)
        .await
        .unwrap();
    assert_eq!(template_pages.len(), 1, "one template page seeded");
    let npc_page = &template_pages[0];
    assert_eq!(npc_page.name, "NPC");

    let block_rows = blocks::Entity::find()
        .filter(blocks::Column::PageId.eq(PageIdCol::from(PageId::from(npc_page.id.clone()))))
        .all(&conn)
        .await
        .unwrap();
    assert!(
        block_rows.len() >= 2,
        "template seeded with multiple blocks"
    );
    // Per-block visibility survives from the `<player_visible>` / `<gm_only>`
    // spans through genesis (the whole point of threading status).
    assert!(
        block_rows
            .iter()
            .any(|b| Status::from(b.status) == Status::Known),
        "a <player_visible> block persists as player-visible",
    );
    assert!(
        block_rows
            .iter()
            .any(|b| Status::from(b.status) == Status::GmOnly),
        "a <gm_only> block persists as gm-only",
    );

    // Stop the supervisor to drain the ToC actor (flushing the folder + nesting).
    supervisor.stop_gracefully().await.unwrap();
    supervisor.wait_for_shutdown_with_result(|_| ()).await;

    let conn = crate::db::connect_readonly(&db_path).await.unwrap();
    let toc = toc_entries::Entity::find().all(&conn).await.unwrap();
    let folder = toc
        .iter()
        .find(|r| r.folder_title.as_deref() == Some("Templates"))
        .expect("Templates folder persisted");
    let npc_entry = toc
        .iter()
        .find(|r| r.page_id.clone().map(PageId::from) == Some(PageId::from(npc_page.id.clone())))
        .expect("npc toc entry persisted");
    assert_eq!(
        npc_entry.parent_id.as_ref(),
        Some(&folder.id),
        "the template page nests under the Templates folder",
    );
}
