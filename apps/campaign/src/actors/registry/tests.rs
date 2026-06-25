use std::sync::Arc;
use std::time::Duration;

use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{ActorRef, Spawn};
use tempfile::TempDir;
use tokio::sync::oneshot;

use super::*;
use crate::actors::relationship_graph::KnownPredicatePairs;
use crate::actors::supervisor::Ping as SupPing;
use crate::db::register_sqlite_vec;
use crate::error::EnsureError;
use crate::persistence::{CampaignStore, LocalCampaignStore};

fn user_id() -> UserId {
    UserId::generate()
}

fn ensure_vec0() {
    register_sqlite_vec();
}

fn store_in(dir: &std::path::Path) -> Arc<dyn CampaignStore> {
    Arc::new(LocalCampaignStore::new(dir.to_path_buf()))
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
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

    let campaign_id = CampaignId::generate();
    let first = registry
        .ask(CreateCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();

    let second = registry
        .ask(CreateCampaign {
            campaign_id: campaign_id.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();

    assert_eq!(first.id(), second.id(), "same supervisor returned twice");
    assert!(tmp.path().join(format!("{}.db", campaign_id.0)).exists());

    begin_drain(&registry).await;
    registry.stop_gracefully().await.unwrap();
    registry.wait_for_shutdown_with_result(|_| ()).await;
}

#[tokio::test]
async fn get_campaign_returns_none_until_ensured() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));
    let id = CampaignId::generate();
    assert!(
        registry
            .ask(GetCampaign(id.clone()))
            .await
            .unwrap()
            .is_none()
    );
    registry
        .ask(CreateCampaign {
            campaign_id: id.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();
    assert!(
        registry
            .ask(GetCampaign(id.clone()))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn get_campaign_handle_graph_ref_is_live() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));
    let id = CampaignId::generate();
    registry
        .ask(CreateCampaign {
            campaign_id: id.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();

    let handle = registry
        .ask(GetCampaign(id.clone()))
        .await
        .unwrap()
        .expect("campaign handle present after create");

    // The cached graph ref answers directly, proving routes can reach the
    // RelationshipGraph without a supervisor hop.
    handle
        .graph
        .ask(KnownPredicatePairs)
        .await
        .expect("relationship graph reachable via the cached ref");
}

#[tokio::test]
async fn supervisor_eviction_removes_from_registry() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_millis(50),
        Duration::from_millis(20),
        None,
    ));

    let id = CampaignId::generate();
    registry
        .ask(CreateCampaign {
            campaign_id: id.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    let after = registry.ask(GetCampaign(id.clone())).await.unwrap();
    assert!(
        after.is_none(),
        "evicted supervisor should be removed from registry"
    );
}

#[tokio::test]
async fn begin_drain_stops_supervisors_in_parallel() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

    let id_a = CampaignId::generate();
    let id_b = CampaignId::generate();
    let sup_a = registry
        .ask(CreateCampaign {
            campaign_id: id_a.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();
    let sup_b = registry
        .ask(CreateCampaign {
            campaign_id: id_b.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();
    sup_a.ask(SupPing).await.unwrap();
    sup_b.ask(SupPing).await.unwrap();

    begin_drain(&registry).await;

    assert!(
        registry
            .ask(GetCampaign(id_a.clone()))
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        registry
            .ask(GetCampaign(id_b.clone()))
            .await
            .unwrap()
            .is_none()
    );
    assert!(sup_a.ask(SupPing).await.is_err());
    assert!(sup_b.ask(SupPing).await.is_err());
}

#[tokio::test]
async fn ensure_propagates_init_failure_and_does_not_insert() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let blocker = tmp.path().join("not-a-dir");
    std::fs::write(&blocker, b"").unwrap();
    let unusable_data_dir = blocker.join("nested");

    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(&unusable_data_dir),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

    let id = CampaignId::generate();
    let err = registry
        .ask(CreateCampaign {
            campaign_id: id.clone(),
            owner_user_id: user_id(),
        })
        .await;
    assert!(err.is_err(), "create should fail when data_dir is unusable");

    assert!(
        registry
            .ask(GetCampaign(id.clone()))
            .await
            .unwrap()
            .is_none(),
        "registry should not retain an entry for a failed init"
    );
}

#[tokio::test]
async fn registry_on_stop_fallback_drains_supervisors() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));
    let supervisor = registry
        .ask(CreateCampaign {
            campaign_id: CampaignId::generate(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();
    registry.stop_gracefully().await.unwrap();
    registry.wait_for_shutdown_with_result(|_| ()).await;
    assert!(
        supervisor.ask(SupPing).await.is_err(),
        "fallback path should have stopped the child supervisor"
    );
}

#[tokio::test]
async fn ask_to_stopped_supervisor_returns_error() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));
    let supervisor = registry
        .ask(CreateCampaign {
            campaign_id: CampaignId::generate(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();
    supervisor.stop_gracefully().await.unwrap();
    supervisor.wait_for_shutdown_with_result(|_| ()).await;
    assert!(supervisor.ask(SupPing).await.is_err());
}

#[tokio::test]
async fn ensure_during_drain_returns_shutting_down() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

    registry
        .ask(CreateCampaign {
            campaign_id: CampaignId::generate(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();

    begin_drain(&registry).await;

    let err = registry
        .ask(CreateCampaign {
            campaign_id: CampaignId::generate(),
            owner_user_id: user_id(),
        })
        .await
        .expect_err("create during drain must error");
    assert!(
        matches!(
            err,
            kameo::error::SendError::HandlerError(EnsureError::ShuttingDown)
        ),
        "expected ShuttingDown, got {err:?}"
    );
}

#[tokio::test]
async fn release_campaign_stops_supervisor() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

    let id = CampaignId::generate();
    let sup = registry
        .ask(CreateCampaign {
            campaign_id: id.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();
    sup.ask(SupPing).await.unwrap();

    registry
        .ask(ReleaseCampaign {
            campaign_id: id.clone(),
        })
        .await
        .unwrap();

    sup.wait_for_shutdown_with_result(|_| ()).await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        registry
            .ask(GetCampaign(id.clone()))
            .await
            .unwrap()
            .is_none(),
        "released supervisor should be removed from registry"
    );
}

#[tokio::test]
async fn release_unknown_campaign_is_noop() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

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
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

    let id_a = CampaignId::generate();
    let id_b = CampaignId::generate();
    registry
        .ask(CreateCampaign {
            campaign_id: id_a.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();
    registry
        .ask(CreateCampaign {
            campaign_id: id_b.clone(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();

    let mut loaded = registry.ask(ListLoaded).await.unwrap();
    loaded.sort_by(|a, b| a.0.0.cmp(&b.0.0));
    assert_eq!(loaded.len(), 2);
    let mut expected = vec![id_a, id_b];
    expected.sort_by(|a, b| a.0.0.cmp(&b.0.0));
    assert_eq!(loaded, expected);
}

#[tokio::test]
async fn list_loaded_returns_empty_during_drain() {
    ensure_vec0();
    let tmp = TempDir::new().unwrap();
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        store_in(tmp.path()),
        Duration::from_secs(60),
        Duration::from_secs(60),
        None,
    ));

    registry
        .ask(CreateCampaign {
            campaign_id: CampaignId::generate(),
            owner_user_id: user_id(),
        })
        .await
        .unwrap();

    begin_drain(&registry).await;

    let loaded = registry.ask(ListLoaded).await.unwrap();
    assert!(loaded.is_empty(), "should return empty during drain");
}
