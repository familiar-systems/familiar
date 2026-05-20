//! `CampaignRegistry`: process-lifetime owner of per-campaign supervisors.
//!
//! The registry is the only path through which HTTP handlers obtain a
//! `CampaignSupervisor` reference. Spawning happens here, in the
//! registry's mailbox, so that:
//!
//! - The map is mutated only from one task.
//! - The kameo `link` is established immediately after spawn, so the
//!   registry's `on_link_died` is the authoritative removal path. When a
//!   supervisor self-evicts on idle (or crashes), the registry observes
//!   the death and removes the entry.
//!
//! Shutdown is decoupled from the registry's mailbox. [`BeginDrain`]
//! sets the registry's [`Phase`] to `Draining`, snapshots the live
//! supervisors, and spawns the drain workflow on the tokio runtime via
//! [`run_drain`]. The handler returns immediately, so the registry can
//! keep replying to incoming queries with `ShuttingDown` instead of
//! blocking its mailbox on supervisor `wait_for_shutdown` futures. The
//! drain task runs all supervisor stops in parallel via [`JoinSet`], so
//! wall time is `max(child_drain_time)` rather than the sum.
//!
//! The idle timeout and eviction check interval are both configured per
//! environment via [`Config`](crate::config::Config). Tests construct
//! the registry with their own short values.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::{Duration, Instant};

use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{Actor, ActorId, ActorRef, Spawn, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use tokio::sync::oneshot;
use tokio::task::JoinSet;

use crate::actors::supervisor::{
    CampaignSupervisor, CampaignSupervisorArgs, SetStopCause, StopCause,
};
use crate::error::EnsureError;
use crate::persistence::CampaignStore;

pub const DRAIN_DEADLINE: Duration = Duration::from_secs(600);

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub enum Phase {
    Ready,
    Draining,
}

pub struct CampaignRegistry {
    supervisors: HashMap<CampaignId, ActorRef<CampaignSupervisor>>,
    phase: Phase,
    store: Arc<dyn CampaignStore>,
    idle_timeout: Duration,
    eviction_check_interval: Duration,
}

impl CampaignRegistry {
    pub fn new(
        store: Arc<dyn CampaignStore>,
        idle_timeout: Duration,
        eviction_check_interval: Duration,
    ) -> Self {
        Self {
            supervisors: HashMap::new(),
            phase: Phase::Ready,
            store,
            idle_timeout,
            eviction_check_interval,
        }
    }
}

impl Actor for CampaignRegistry {
    type Args = Self;
    type Error = std::convert::Infallible;

    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        tracing::info!(
            idle_timeout_secs = args.idle_timeout.as_secs(),
            eviction_check_interval_ms = args.eviction_check_interval.as_millis() as u64,
            "campaign registry started"
        );
        Ok(args)
    }

    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        _reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        let count = self.supervisors.len();
        if count > 0 {
            tracing::warn!(
                supervisor_count = count,
                "registry stopping without explicit BeginDrain; cleaning up children synchronously"
            );
            for (_id, sup_ref) in self.supervisors.drain() {
                let _ = sup_ref
                    .tell(SetStopCause(StopCause::RegistryFallback))
                    .await;
                let _ = sup_ref.stop_gracefully().await;
                sup_ref.wait_for_shutdown_with_result(|_| ()).await;
            }
        }
        Ok(())
    }

    async fn on_link_died(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        id: ActorId,
        reason: ActorStopReason,
    ) -> Result<ControlFlow<ActorStopReason>, Self::Error> {
        let before = self.supervisors.len();
        self.supervisors.retain(|_, sup_ref| sup_ref.id() != id);
        let removed = before - self.supervisors.len();
        if removed > 0 {
            tracing::debug!(
                ?reason,
                supervisor_count = self.supervisors.len(),
                "supervisor removed from registry via link_died"
            );
        }
        Ok(ControlFlow::Continue(()))
    }
}

#[derive(Debug, Clone)]
pub struct EnsureCampaign {
    pub campaign_id: CampaignId,
    pub owner_user_id: UserId,
}

impl Message<EnsureCampaign> for CampaignRegistry {
    type Reply = Result<ActorRef<CampaignSupervisor>, EnsureError>;

    async fn handle(
        &mut self,
        msg: EnsureCampaign,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if matches!(self.phase, Phase::Draining) {
            tracing::debug!(
                campaign_id = %msg.campaign_id.0,
                "rejecting ensure during drain"
            );
            return Err(EnsureError::ShuttingDown);
        }

        if let Some(existing) = self.supervisors.get(&msg.campaign_id) {
            tracing::debug!(
                campaign_id = %msg.campaign_id.0,
                "supervisor already running"
            );
            return Ok(existing.clone());
        }

        tracing::info!(
            campaign_id = %msg.campaign_id.0,
            owner_user_id = %msg.owner_user_id.0,
            "spawning campaign supervisor"
        );

        let started = Instant::now();

        let supervisor = CampaignSupervisor::spawn(CampaignSupervisorArgs {
            campaign_id: msg.campaign_id.clone(),
            store: self.store.clone(),
            idle_timeout: self.idle_timeout,
            eviction_check_interval: self.eviction_check_interval,
        });

        ctx.actor_ref().link(&supervisor).await;

        supervisor.wait_for_startup().await;
        if !supervisor.is_alive() {
            tracing::warn!(
                campaign_id = %msg.campaign_id.0,
                "supervisor died during startup"
            );
            return Err(EnsureError::SupervisorDied);
        }

        self.supervisors
            .insert(msg.campaign_id.clone(), supervisor.clone());

        tracing::info!(
            campaign_id = %msg.campaign_id.0,
            init_total_elapsed_ms = started.elapsed().as_millis() as u64,
            "campaign ensured"
        );
        Ok(supervisor)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GetPhase;

impl Message<GetPhase> for CampaignRegistry {
    type Reply = Phase;

    async fn handle(&mut self, _: GetPhase, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.phase
    }
}

#[derive(Debug, Clone)]
pub struct GetCampaign(pub CampaignId);

impl Message<GetCampaign> for CampaignRegistry {
    type Reply = Option<ActorRef<CampaignSupervisor>>;

    async fn handle(
        &mut self,
        GetCampaign(id): GetCampaign,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if matches!(self.phase, Phase::Draining) {
            return None;
        }
        self.supervisors.get(&id).cloned()
    }
}

pub struct BeginDrain {
    pub completion: oneshot::Sender<()>,
}

impl Message<BeginDrain> for CampaignRegistry {
    type Reply = ();

    async fn handle(&mut self, msg: BeginDrain, _ctx: &mut Context<Self, Self::Reply>) {
        if matches!(self.phase, Phase::Draining) {
            tracing::debug!("BeginDrain received during drain; signalling caller immediately");
            let _ = msg.completion.send(());
            return;
        }
        self.phase = Phase::Draining;

        let snapshot: Vec<_> = self.supervisors.drain().collect();
        let count = snapshot.len();
        tracing::info!(
            supervisor_count = count,
            "drain initiated, spawning workflow"
        );

        tokio::spawn(run_drain(snapshot, msg.completion));
    }
}

async fn run_drain(
    snapshot: Vec<(CampaignId, ActorRef<CampaignSupervisor>)>,
    completion: oneshot::Sender<()>,
) {
    let count = snapshot.len();
    let started = Instant::now();

    for (id, sup) in &snapshot {
        let _ = sup.tell(SetStopCause(StopCause::Drain)).await;
        if let Err(e) = sup.stop_gracefully().await {
            tracing::warn!(
                ?e,
                campaign_id = %id.0,
                actor_kind = "campaign_supervisor",
                "actor already stopped during drain"
            );
        }
    }

    let mut set: JoinSet<CampaignId> = JoinSet::new();
    for (id, sup) in &snapshot {
        let id = id.clone();
        let sup = sup.clone();
        set.spawn(async move {
            sup.wait_for_shutdown_with_result(|_| ()).await;
            id
        });
    }

    let drain_fut = async {
        while let Some(res) = set.join_next().await {
            match res {
                Ok(id) => tracing::debug!(campaign_id = %id.0, "supervisor drained"),
                Err(join_err) => tracing::warn!(?join_err, "drain task panicked or was aborted"),
            }
        }
    };

    match tokio::time::timeout(DRAIN_DEADLINE, drain_fut).await {
        Ok(()) => {
            tracing::info!(
                supervisor_count = count,
                drain_elapsed_ms = started.elapsed().as_millis() as u64,
                "all campaigns drained"
            );
        }
        Err(_) => {
            tracing::error!(
                supervisor_count = count,
                deadline_secs = DRAIN_DEADLINE.as_secs(),
                elapsed_secs = started.elapsed().as_secs(),
                "drain deadline exceeded; killing all supervisors"
            );
            for (_id, sup) in &snapshot {
                sup.kill();
            }
            set.abort_all();
        }
    }

    let _ = completion.send(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::supervisor::Ping as SupPing;
    use crate::db::register_sqlite_vec;
    use crate::persistence::LocalCampaignStore;
    use tempfile::TempDir;

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
        ));

        let campaign_id = CampaignId::generate();
        let first = registry
            .ask(EnsureCampaign {
                campaign_id: campaign_id.clone(),
                owner_user_id: user_id(),
            })
            .await
            .unwrap();

        let second = registry
            .ask(EnsureCampaign {
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
            .ask(EnsureCampaign {
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
    async fn supervisor_eviction_removes_from_registry() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let registry = CampaignRegistry::spawn(CampaignRegistry::new(
            store_in(tmp.path()),
            Duration::from_millis(50),
            Duration::from_millis(20),
        ));

        let id = CampaignId::generate();
        registry
            .ask(EnsureCampaign {
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
        ));

        let id_a = CampaignId::generate();
        let id_b = CampaignId::generate();
        let sup_a = registry
            .ask(EnsureCampaign {
                campaign_id: id_a.clone(),
                owner_user_id: user_id(),
            })
            .await
            .unwrap();
        let sup_b = registry
            .ask(EnsureCampaign {
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
        ));

        let id = CampaignId::generate();
        let err = registry
            .ask(EnsureCampaign {
                campaign_id: id.clone(),
                owner_user_id: user_id(),
            })
            .await;
        assert!(err.is_err(), "ensure should fail when data_dir is unusable");

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
        ));
        let supervisor = registry
            .ask(EnsureCampaign {
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
        ));
        let supervisor = registry
            .ask(EnsureCampaign {
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
        ));

        registry
            .ask(EnsureCampaign {
                campaign_id: CampaignId::generate(),
                owner_user_id: user_id(),
            })
            .await
            .unwrap();

        begin_drain(&registry).await;

        let err = registry
            .ask(EnsureCampaign {
                campaign_id: CampaignId::generate(),
                owner_user_id: user_id(),
            })
            .await
            .expect_err("ensure during drain must error");
        assert!(
            matches!(
                err,
                kameo::error::SendError::HandlerError(EnsureError::ShuttingDown)
            ),
            "expected ShuttingDown, got {err:?}"
        );
    }
}
