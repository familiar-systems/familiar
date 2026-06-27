use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{ActorRef, Spawn};
use kameo::error::SendError;
use tempfile::TempDir;
use tokio::sync::oneshot;

use super::*;
use crate::actors::relationship_graph::KnownPredicatePairs;
use crate::actors::supervisor::Ping as SupPing;
use crate::db::register_sqlite_vec;
use crate::error::{EnsureError, ResolveError};
use crate::persistence::{CampaignStore, LocalCampaignStore};

const READY: Duration = Duration::from_secs(30);

fn user_id() -> UserId {
    UserId::generate()
}

fn ensure_vec0() {
    register_sqlite_vec();
}

fn store_in(dir: &std::path::Path) -> Arc<dyn CampaignStore> {
    Arc::new(LocalCampaignStore::new(dir.to_path_buf()))
}

fn new_table() -> CampaignTable {
    Arc::new(ArcSwap::from_pointee(HashMap::new()))
}

/// Spawn a registry over a fresh table, returning both so tests can read the
/// routing table directly (the registry is its only writer).
fn spawn_registry(
    store: Arc<dyn CampaignStore>,
    idle: Duration,
    eviction: Duration,
) -> (ActorRef<CampaignRegistry>, CampaignTable) {
    let table = new_table();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        table.clone(),
        store,
        idle,
        eviction,
        None,
    ));
    (registry, table)
}

fn peek(table: &CampaignTable, id: &CampaignId) -> Option<CampaignState> {
    table.load().get(id).cloned()
}

/// Checkout is async: `CreateCampaign` returns a `Loading` state immediately.
/// This drives a campaign to `Ready` the way a route does (ask, then resolve).
async fn create_ready(
    registry: &ActorRef<CampaignRegistry>,
    id: CampaignId,
    owner: UserId,
) -> CampaignHandle {
    let state = registry
        .ask(CreateCampaign {
            campaign_id: id,
            owner_user_id: owner,
        })
        .await
        .expect("create send");
    resolve(Some(state), READY).await.expect("checkout ready")
}

/// Removal (idle eviction, release, drain) happens via `on_link_died` /
/// `LoadComplete`, which are async relative to the caller. Poll until gone.
async fn wait_until_absent(table: &CampaignTable, id: &CampaignId, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if peek(table, id).is_none() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("entry for {} still present after {timeout:?}", id.0);
}

async fn begin_drain(registry: &ActorRef<CampaignRegistry>) {
    let (tx, rx) = oneshot::channel();
    registry
        .ask(BeginDrain { completion: tx })
        .await
        .expect("BeginDrain send");
    rx.await.expect("drain completion");
}

#[tokio::test]
async fn ensure_campaign_spawns_and_is_idempotent() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    let campaign_id = CampaignId::generate();
    let first = create_ready(&registry, campaign_id.clone(), user_id()).await;
    let second = create_ready(&registry, campaign_id.clone(), user_id()).await;

    assert_eq!(
        first.supervisor.id(),
        second.supervisor.id(),
        "same supervisor returned twice"
    );
    assert!(tmp.path().join(format!("{}.db", campaign_id.0)).exists());

    begin_drain(&registry).await;
    registry.stop_gracefully().await.unwrap();
    registry.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn table_absent_until_checked_out_then_ready() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );
    let id = CampaignId::generate();
    assert!(peek(&table, &id).is_none());

    create_ready(&registry, id.clone(), user_id()).await;
    assert!(matches!(peek(&table, &id), Some(CampaignState::Ready(_))));
}

#[tokio::test]
async fn handle_graph_ref_is_live() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );
    let handle = create_ready(&registry, CampaignId::generate(), user_id()).await;

    // The cached graph ref answers directly, proving routes can reach the
    // RelationshipGraph without a supervisor hop.
    handle
        .graph
        .ask(KnownPredicatePairs)
        .await
        .expect("relationship graph reachable via the cached ref");
}

#[tokio::test]
async fn concurrent_ensures_coalesce_on_one_load() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );
    let id = CampaignId::generate();

    // Fire several ensures for the same campaign before the first finishes
    // loading; they must all resolve to the SAME supervisor (one spawn).
    let mut joins = Vec::new();
    for _ in 0..5 {
        let registry = registry.clone();
        let id = id.clone();
        joins.push(tokio::spawn(async move {
            let state = registry
                .ask(EnsureCampaign { campaign_id: id })
                .await
                .expect("ensure send");
            resolve(Some(state), READY).await.expect("ready")
        }));
    }

    let mut ids = Vec::new();
    for j in joins {
        ids.push(j.await.unwrap().supervisor.id());
    }
    assert!(
        ids.windows(2).all(|w| w[0] == w[1]),
        "all coalesced ensures must yield the same supervisor, got {ids:?}"
    );
}

#[tokio::test]
async fn supervisor_eviction_removes_from_table() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_millis(50),
        Duration::from_millis(20),
    );

    let id = CampaignId::generate();
    create_ready(&registry, id.clone(), user_id()).await;

    wait_until_absent(&table, &id, Duration::from_secs(2)).await;
}

#[tokio::test]
async fn begin_drain_stops_supervisors_in_parallel() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    let id_a = CampaignId::generate();
    let id_b = CampaignId::generate();
    let sup_a = create_ready(&registry, id_a.clone(), user_id()).await;
    let sup_b = create_ready(&registry, id_b.clone(), user_id()).await;
    sup_a.supervisor.ask(SupPing).await.unwrap();
    sup_b.supervisor.ask(SupPing).await.unwrap();

    begin_drain(&registry).await;

    assert!(sup_a.supervisor.ask(SupPing).await.is_err());
    assert!(sup_b.supervisor.ask(SupPing).await.is_err());
    wait_until_absent(&table, &id_a, Duration::from_secs(2)).await;
    wait_until_absent(&table, &id_b, Duration::from_secs(2)).await;
}

#[tokio::test]
async fn failed_load_resolves_failed_and_removes_entry() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let blocker = tmp.path().join("not-a-dir");
    std::fs::write(&blocker, b"").unwrap();
    let unusable_data_dir = blocker.join("nested");

    let (registry, table) = spawn_registry(
        store_in(&unusable_data_dir),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    let id = CampaignId::generate();
    let state = registry
        .ask(CreateCampaign {
            campaign_id: id.clone(),
            owner_user_id: user_id(),
        })
        .await
        .expect("create send (handler succeeds; the load fails async)");

    // The async load hits an init failure -> resolve surfaces LoadFailed...
    assert!(matches!(
        resolve(Some(state), READY).await,
        Err(ResolveError::LoadFailed)
    ));
    // ...and the failed entry is dropped so a retry can re-checkout.
    wait_until_absent(&table, &id, Duration::from_secs(2)).await;
}

#[tokio::test]
async fn registry_on_stop_fallback_drains_supervisors() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );
    let handle = create_ready(&registry, CampaignId::generate(), user_id()).await;

    registry.stop_gracefully().await.unwrap();
    registry.wait_for_shutdown_with_result(|_| ()).await;
    assert!(
        handle.supervisor.ask(SupPing).await.is_err(),
        "fallback path should have stopped the child supervisor"
    );
}

#[tokio::test]
async fn ask_to_stopped_supervisor_returns_error() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );
    let handle = create_ready(&registry, CampaignId::generate(), user_id()).await;

    handle.supervisor.stop_gracefully().await.unwrap();
    handle
        .supervisor
        .wait_for_shutdown_with_result(|_| ())
        .await;
    assert!(handle.supervisor.ask(SupPing).await.is_err());
}

#[tokio::test]
async fn ensure_during_drain_returns_shutting_down() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    create_ready(&registry, CampaignId::generate(), user_id()).await;
    begin_drain(&registry).await;

    let result = registry
        .ask(CreateCampaign {
            campaign_id: CampaignId::generate(),
            owner_user_id: user_id(),
        })
        .await;
    assert!(
        matches!(
            result,
            Err(SendError::HandlerError(EnsureError::ShuttingDown))
        ),
        "expected ShuttingDown during drain"
    );
}

#[tokio::test]
async fn release_campaign_stops_supervisor() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    let id = CampaignId::generate();
    let handle = create_ready(&registry, id.clone(), user_id()).await;
    handle.supervisor.ask(SupPing).await.unwrap();

    registry
        .ask(ReleaseCampaign {
            campaign_id: id.clone(),
        })
        .await
        .unwrap();

    handle
        .supervisor
        .wait_for_shutdown_with_result(|_| ())
        .await;
    wait_until_absent(&table, &id, Duration::from_secs(2)).await;
}

#[tokio::test]
async fn release_unknown_campaign_is_noop() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    registry
        .ask(ReleaseCampaign {
            campaign_id: CampaignId::generate(),
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn list_loaded_returns_campaign_ids() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    let id_a = CampaignId::generate();
    let id_b = CampaignId::generate();
    create_ready(&registry, id_a.clone(), user_id()).await;
    create_ready(&registry, id_b.clone(), user_id()).await;

    let mut loaded = registry.ask(ListLoaded).await.unwrap();
    loaded.sort_by(|a, b| a.0.0.cmp(&b.0.0));
    let mut expected = vec![id_a, id_b];
    expected.sort_by(|a, b| a.0.0.cmp(&b.0.0));
    assert_eq!(loaded, expected);
}

#[tokio::test]
async fn list_loaded_returns_empty_during_drain() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let (registry, _table) = spawn_registry(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
    );

    create_ready(&registry, CampaignId::generate(), user_id()).await;
    begin_drain(&registry).await;

    let loaded = registry.ask(ListLoaded).await.unwrap();
    assert!(loaded.is_empty(), "should return empty during drain");
}
