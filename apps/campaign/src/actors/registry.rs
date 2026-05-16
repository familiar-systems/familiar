//! `CampaignRegistry`: process-lifetime owner of per-campaign supervisors.
//!
//! The registry is the only path through which HTTP handlers obtain a
//! `CampaignSupervisor` reference. Spawning happens here, in the
//! registry's mailbox, so that:
//!
//! - Storage init (open SQLite pool, run migrations) is serialized against
//!   other ensures, eliminating concurrent-create races.
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
use std::path::PathBuf;
use std::time::{Duration, Instant};

use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{Actor, ActorId, ActorRef, Spawn, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use tokio::sync::oneshot;
use tokio::task::JoinSet;

use crate::actors::supervisor::{
    CampaignSupervisor, CampaignSupervisorArgs, SetStopCause, StopCause,
    initialize_campaign_storage,
};
use crate::error::EnsureError;

/// Hard cap on how long a drain workflow waits for in-flight supervisor
/// shutdowns before force-killing the laggards. Picked at 10 minutes to
/// accommodate larger bucket uploads in the future without hanging
/// deploys; expected real drains finish in seconds. Past this deadline,
/// every supervisor receives `kill()` (a no-op on those that already
/// stopped); their `on_stop` hooks still run per kameo's contract, so
/// partial cleanup is best-effort.
pub const DRAIN_DEADLINE: Duration = Duration::from_secs(600);

/// Lifecycle phase of the registry. Transitions are strictly one-way:
/// `Ready` is the only state in which new campaigns are spawned, and
/// `Draining` is terminal for the lifetime of this registry instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub enum Phase {
    Ready,
    Draining,
}

pub struct CampaignRegistry {
    supervisors: HashMap<CampaignId, ActorRef<CampaignSupervisor>>,
    phase: Phase,
    data_dir: PathBuf,
    idle_timeout: Duration,
    eviction_check_interval: Duration,
}

impl CampaignRegistry {
    pub fn new(
        data_dir: PathBuf,
        idle_timeout: Duration,
        eviction_check_interval: Duration,
    ) -> Self {
        Self {
            supervisors: HashMap::new(),
            phase: Phase::Ready,
            data_dir,
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
            data_dir = %args.data_dir.display(),
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
        // BeginDrain is the standard shutdown path; on_stop is a fallback
        // for the case where the registry stops without an explicit drain
        // (programmer error, or a panic). Stop every still-tracked
        // supervisor sequentially. This is the slow path and only fires
        // if shutdown was misordered upstream. The `RegistryFallback`
        // cause tag lets log readers tell this apart from the orderly
        // drain path.
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
        // Never propagate a child's stop reason to the registry. A single
        // dead campaign should not take down the whole process.
        Ok(ControlFlow::Continue(()))
    }
}

/// Idempotent: spawn a `CampaignSupervisor` for `campaign_id` if one
/// isn't already live, run storage init, register it, and return its
/// `ActorRef`. Repeat calls with the same `campaign_id` return the
/// existing ref. Returns [`EnsureError::ShuttingDown`] if the registry
/// has already entered its drain phase.
///
/// `owner_user_id` is plumbed through to logging only; persistence
/// arrives with the users table in the auth-membership work and is
/// not yet wired at the time of writing.
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
        let (db_path, conn) = initialize_campaign_storage(&msg.campaign_id, &self.data_dir).await?;

        let supervisor = CampaignSupervisor::spawn(CampaignSupervisorArgs {
            campaign_id: msg.campaign_id.clone(),
            db_path,
            conn,
            idle_timeout: self.idle_timeout,
            eviction_check_interval: self.eviction_check_interval,
        });

        // Link bidirectionally so the registry sees on_link_died when
        // the supervisor stops for any reason (idle eviction, crash,
        // signal drain). `link` is the only kameo path that fires when
        // a child stops; on_stop on the child runs but doesn't carry
        // back to the parent.
        //
        // FIXME: there is a tiny window between `spawn` and `link.await`
        // during which the supervisor's actor task is running. Today
        // that window is irrelevant because `on_start` is `Infallible`
        // (it cannot exit before reaching the message loop), but once
        // the bucket-world on_start move (see the FIXME in
        // `supervisor::CampaignSupervisor::on_start`) lands, `on_start`
        // can return `Err(InitError)`. If on_start fails during this
        // window, the actor stops before `link` records the parent.
        // on_link_died won't fire, leaving a stale ref in the map until
        // an external signal (next ensure for the same id, drain, etc.)
        // surfaces it.
        //
        // Fix when the bucket move lands: either link inside the actor's
        // own task (`on_start` calls a "link to parent" hook before
        // doing the slow work) so the link is established before any
        // failure can race it, or detect post-link that the actor is
        // already dead via `is_alive()`/`wait_for_startup_result()` and
        // clean up the map entry inline.
        ctx.actor_ref().link(&supervisor).await;

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

/// Returns the registry's current [`Phase`]. The HTTP `/health`
/// handler uses this to map readiness state to a status code (200 for
/// Ready, 503 for Draining). The k8s readiness probe consumes that
/// status to take the pod out of the LB rotation as soon as drain
/// begins.
#[derive(Debug, Clone, Copy)]
pub struct GetPhase;

impl Message<GetPhase> for CampaignRegistry {
    type Reply = Phase;

    async fn handle(&mut self, _: GetPhase, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.phase
    }
}

/// Lookup without spawning. Returns `None` if no supervisor is currently
/// live for `campaign_id`, or if the registry is in its drain phase.
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

/// Sent by `main` after axum drains. Transitions the registry into the
/// `Draining` phase and spawns [`run_drain`] on the tokio runtime. The
/// handler returns immediately; the caller awaits `completion` to learn
/// when the drain workflow has actually finished.
///
/// Sending `BeginDrain` while a drain is already in progress signals the
/// new completion sender right away. This is acceptable because the
/// production caller (`main.rs`) only ever sends one `BeginDrain`; the
/// fast-path here exists so that a misbehaving caller doesn't hang
/// forever waiting on a oneshot that nobody ever resolves.
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

        // `drain` takes ownership of every (id, ref) pair, leaving the
        // map empty. on_link_died notifications still fire during the
        // drain but find nothing to remove, which is correct.
        let snapshot: Vec<_> = self.supervisors.drain().collect();
        let count = snapshot.len();
        tracing::info!(
            supervisor_count = count,
            "drain initiated, spawning workflow"
        );

        tokio::spawn(run_drain(snapshot, msg.completion));
    }
}

/// The drain workflow itself. Lives on the tokio runtime, not in the
/// registry's mailbox, so the registry stays responsive while drain runs.
///
/// Phase 1 tags each supervisor with `StopCause::Drain` (so its on_stop
/// log reads `cause=drain`) then fires `stop_gracefully` on every
/// supervisor (just queues a Stop signal, returns immediately).
///
/// Phase 2 awaits every supervisor's shutdown concurrently via
/// [`JoinSet`], wrapped in a [`DRAIN_DEADLINE`] timeout so a single
/// stuck supervisor can't hang the deploy past the k8s grace period.
/// On timeout, every supervisor in the snapshot is `kill()`-ed (a
/// no-op for those that already stopped) and the join futures are
/// abort_all'd.
async fn run_drain(
    snapshot: Vec<(CampaignId, ActorRef<CampaignSupervisor>)>,
    completion: oneshot::Sender<()>,
) {
    let count = snapshot.len();
    let started = Instant::now();

    for (id, sup) in &snapshot {
        // Best-effort cause-tagging. If the supervisor's mailbox is
        // already closed (it stopped from idle eviction milliseconds
        // ago), the tell errors and the cause stays whatever the
        // supervisor set itself; that's fine, the supervisor's own
        // cause is still accurate.
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
                "drain deadline exceeded; killing all supervisors (no-op on those already stopped)"
            );
            for (_id, sup) in &snapshot {
                sup.kill();
            }
            // Cancel any still-pending join futures; we no longer care
            // about their result. on_stop still runs for the killed
            // supervisors per kameo's contract.
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
    use tempfile::TempDir;

    fn user_id() -> UserId {
        UserId::generate()
    }

    /// See the matching helper in `supervisor::tests`. Migrations open a
    /// `vec0` table; the extension must be auto-registered before any
    /// campaign DB opens. Idempotent.
    fn ensure_vec0() {
        register_sqlite_vec();
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
            tmp.path().to_path_buf(),
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
            tmp.path().to_path_buf(),
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
            tmp.path().to_path_buf(),
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

        // Wait long enough for the supervisor to idle out and for the
        // link_died notification to propagate back to the registry.
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
            tmp.path().to_path_buf(),
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
        // Both supervisors are reachable before drain.
        sup_a.ask(SupPing).await.unwrap();
        sup_b.ask(SupPing).await.unwrap();

        begin_drain(&registry).await;

        // After drain completes, GetCampaign must short-circuit on the
        // Draining phase and return None, and direct sends to the now-
        // stopped supervisors must fail.
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
        // Construct a data_dir that `create_dir_all` cannot satisfy:
        // create a regular file, then point the registry at a path
        // *inside* that file. `tokio::fs::create_dir_all` returns
        // NotADirectory, which `initialize_campaign_storage` maps to
        // `InitError::CreateDir`, which the EnsureCampaign reply
        // folds into `SendError::HandlerError(EnsureError::Init(...))`.
        let tmp = TempDir::new().unwrap();
        let blocker = tmp.path().join("not-a-dir");
        std::fs::write(&blocker, b"").unwrap();
        let unusable_data_dir = blocker.join("nested");

        let registry = CampaignRegistry::spawn(CampaignRegistry::new(
            unusable_data_dir,
            Duration::from_secs(60),
            Duration::from_secs(60),
        ));

        let id = CampaignId::generate();
        let err = registry
            .ask(EnsureCampaign {
                campaign_id: id.clone(),
                owner_user_id: user_id(),
            })
            .await
            .expect_err("ensure should fail when data_dir is unusable");
        assert!(
            matches!(
                err,
                kameo::error::SendError::HandlerError(EnsureError::Init(
                    crate::error::InitError::CreateDir { .. }
                ))
            ),
            "expected Init(CreateDir), got {err:?}"
        );

        // The failed ensure must not leave a stale entry in the map.
        // A subsequent GetCampaign for the same id returns None.
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
            tmp.path().to_path_buf(),
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
        // Stop the registry directly, skipping BeginDrain. The fallback
        // path in CampaignRegistry::on_stop must still cascade the stop
        // to every child supervisor; this guards against a regression
        // that silently leaves orphaned supervisors when shutdown
        // ordering is wrong upstream.
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
        // The plan accepts a rare race where an HTTP handler holds a
        // supervisor `ActorRef` and the supervisor evicts itself before
        // the handler sends. Verifying it deterministically: stop the
        // supervisor, wait for full shutdown, then send. The expected
        // behaviour is a transport error the HTTP handler maps to 503.
        let tmp = TempDir::new().unwrap();
        let registry = CampaignRegistry::spawn(CampaignRegistry::new(
            tmp.path().to_path_buf(),
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
            tmp.path().to_path_buf(),
            Duration::from_secs(60),
            Duration::from_secs(60),
        ));

        // Seed one supervisor so the drain has something to do.
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
