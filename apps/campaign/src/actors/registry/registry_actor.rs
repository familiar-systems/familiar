//! `CampaignRegistry`: process-lifetime owner of the shard's routing table.
//!
//! The registry maps each checked-out campaign to a [`CampaignState`] in a
//! lock-free [`CampaignTable`] (an [`ArcSwap`] snapshot map). HTTP/WS handlers
//! hold a clone of that `Arc` and read snapshots wait-free; **the registry
//! actor is the only writer** — every mutation flows through its mailbox
//! (message handlers + `on_link_died`), so the RCU (load -> clone -> store)
//! never races. Concurrent writers would lose updates, which is exactly why
//! the detached load driver never touches the table itself: it publishes to a
//! `watch` channel and asks the registry to do the table write.
//!
//! ## Async checkout
//!
//! Checkout is non-blocking. On a miss the handler spawns the supervisor,
//! links it, inserts a [`CampaignState::Loading`] entry, spawns a detached
//! driver, and returns immediately — the registry mailbox never awaits a
//! supervisor's `on_start` (which can take seconds once the store is S3).
//! Concurrent checkouts of the same campaign coalesce on the one `Loading`
//! entry's `watch`. The driver runs `wait_for_startup` off-mailbox, then asks
//! the registry to flip `Loading -> Ready` (or remove on failure).
//!
//! ## Shutdown
//!
//! [`BeginDrain`] flips the registry [`Phase`] to `Draining`, marks every
//! entry `Draining`, snapshots the supervisors, and spawns `run_drain` on the
//! tokio runtime. The handler returns immediately so the registry keeps
//! replying while children stop in parallel via [`JoinSet`] (wall time
//! `max(child_drain_time)`, not the sum). Per-campaign `ReleaseCampaign`
//! marks just that entry `Draining`. In both cases `on_link_died` removes the
//! entry once the supervisor finishes.
//!
//! The idle timeout and eviction check interval are configured per environment
//! via [`Config`](crate::config::Config). Tests construct the registry with
//! their own short values and their own [`CampaignTable`] handle.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{Actor, ActorId, ActorRef, Spawn, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use tokio::sync::{oneshot, watch};
use tokio::task::JoinSet;

use crate::actors::relationship_graph::RelationshipGraph;
use crate::actors::supervisor::{
    CampaignSupervisor, CampaignSupervisorArgs, RelationshipGraphRef, SetStopCause, StopCause,
};
use crate::clients::platform_internal::PlatformInternalClient;
use crate::error::{EnsureError, ResolveError};
use crate::persistence::CampaignStore;

pub const DRAIN_DEADLINE: Duration = Duration::from_secs(600);

/// Bounded wait for an in-flight checkout before a request gives up. A
/// stopgap: a request that lands mid-load blocks up to this long, then 503s
/// (the load continues in the background, so a retry usually succeeds). This
/// is the seam for a real loading-screen / progress UX later.
pub const READY_WAIT_TIMEOUT: Duration = Duration::from_secs(30);

/// References to actors needed for 'external' communication.
/// - `supervisor`: the campaign supervisor actor, handling overall campaign lifecycle
/// - `graph`: the relationship graph actor, handling all relationship mutations
///
/// Everything is technically accessible through the supervisor.
/// However, it's more efficient to just call subactors directly.
#[derive(Clone, kameo::Reply)]
pub struct CampaignHandle {
    pub supervisor: ActorRef<CampaignSupervisor>,
    pub graph: ActorRef<RelationshipGraph>,
}

/// Terminal-or-pending result an awaiter reads from a [`CampaignState::Loading`]
/// entry's `watch`. The driver task publishes `Ready`/`Failed`; the registry
/// separately flips the table entry to [`CampaignState::Ready`] (or removes it).
pub enum LoadOutcome {
    Pending,
    Ready(CampaignHandle),
    Failed,
}

/// An in-flight checkout. `rx` lets any number of waiters await the load's
/// terminal outcome; `supervisor` lets `on_link_died` match this entry to its
/// (linked) supervisor and lets the load-completion guard reject a stale flip.
#[derive(Clone)]
pub struct LoadingEntry {
    pub rx: watch::Receiver<LoadOutcome>,
    pub supervisor: ActorRef<CampaignSupervisor>,
}

/// One campaign's slot in the [`CampaignTable`].
#[derive(Clone, kameo::Reply)]
pub enum CampaignState {
    /// Checkout in flight. Awaiters resolve via the `watch`.
    Loading(LoadingEntry),
    /// Live and serving.
    Ready(CampaignHandle),
    /// Being torn down (platform release or shard drain). Reads 503 until
    /// `on_link_died` removes the entry.
    Draining {
        supervisor: ActorRef<CampaignSupervisor>,
    },
}

impl CampaignState {
    /// The supervisor backing this slot, in any state. Lets `on_link_died` and
    /// the drain paths address the actor without matching on the variant.
    pub fn supervisor(&self) -> &ActorRef<CampaignSupervisor> {
        match self {
            CampaignState::Loading(e) => &e.supervisor,
            CampaignState::Ready(h) => &h.supervisor,
            CampaignState::Draining { supervisor } => supervisor,
        }
    }
}

/// The shard's routing table: a lock-free snapshot map from campaign to its
/// checkout state. The registry actor is the only writer (see module docs);
/// route handlers hold a clone of this `Arc` and read snapshots wait-free.
pub type CampaignTable = Arc<ArcSwap<HashMap<CampaignId, CampaignState>>>;

/// Resolve a routing-table entry to a live handle, awaiting an in-flight load
/// up to `timeout`. Shared by the direct-read path (public GM routes read the
/// `ArcSwap` snapshot) and the ensure path (WS/internal await the reply).
pub async fn resolve(
    state: Option<CampaignState>,
    timeout: Duration,
) -> Result<CampaignHandle, ResolveError> {
    match state {
        None => Err(ResolveError::NotLoaded),
        Some(CampaignState::Ready(handle)) => Ok(handle),
        Some(CampaignState::Draining { .. }) => Err(ResolveError::Draining),
        Some(CampaignState::Loading(mut entry)) => {
            let waited = tokio::time::timeout(
                timeout,
                entry.rx.wait_for(|o| !matches!(o, LoadOutcome::Pending)),
            )
            .await;
            match waited {
                Ok(Ok(guard)) => match &*guard {
                    LoadOutcome::Ready(handle) => Ok(handle.clone()),
                    // `Failed`, or `Pending` (unreachable: `wait_for` only
                    // returns when the predicate holds) -> treat as a failed
                    // load rather than panicking.
                    LoadOutcome::Failed | LoadOutcome::Pending => Err(ResolveError::LoadFailed),
                },
                // Sender dropped without a terminal value (driver panicked).
                Ok(Err(_)) => Err(ResolveError::LoadFailed),
                Err(_elapsed) => Err(ResolveError::StillLoading),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub enum Phase {
    Ready,
    Draining,
}

pub struct CampaignRegistry {
    table: CampaignTable,
    phase: Phase,
    store: Arc<dyn CampaignStore>,
    idle_timeout: Duration,
    eviction_check_interval: Duration,
    platform_client: Option<PlatformInternalClient>,
}

impl CampaignRegistry {
    pub fn new(
        table: CampaignTable,
        store: Arc<dyn CampaignStore>,
        idle_timeout: Duration,
        eviction_check_interval: Duration,
        platform_client: Option<PlatformInternalClient>,
    ) -> Self {
        Self {
            table,
            phase: Phase::Ready,
            store,
            idle_timeout,
            eviction_check_interval,
            platform_client,
        }
    }

    /// Single-writer RCU on the routing table. Only the registry actor's task
    /// calls this, so a plain load -> clone -> store needs no compare-and-swap
    /// retry loop.
    fn write_table<R>(&self, f: impl FnOnce(&mut HashMap<CampaignId, CampaignState>) -> R) -> R {
        let mut map = HashMap::clone(&self.table.load());
        let out = f(&mut map);
        self.table.store(Arc::new(map));
        out
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
        let states: Vec<CampaignState> = self.table.load().values().cloned().collect();
        if !states.is_empty() {
            tracing::warn!(
                supervisor_count = states.len(),
                "registry stopping without explicit BeginDrain; cleaning up children synchronously"
            );
            for state in states {
                let sup = state.supervisor().clone();
                let _ = sup.tell(SetStopCause(StopCause::RegistryFallback)).await;
                let _ = sup.stop_gracefully().await;
                sup.wait_for_shutdown_with_result(|_| ()).await;
            }
            self.write_table(|map| map.clear());
        }
        Ok(())
    }

    async fn on_link_died(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        id: ActorId,
        reason: ActorStopReason,
    ) -> Result<ControlFlow<ActorStopReason>, Self::Error> {
        let removed = self.write_table(|map| {
            let before = map.len();
            map.retain(|_, st| st.supervisor().id() != id);
            before != map.len()
        });
        if removed {
            tracing::debug!(?reason, "supervisor removed from registry via link_died");
        }
        // We learn that a supervisor has drained only here, via the kameo
        // link, rather than having the supervisor pre-notify the registry.
        // That keeps the supervisor decoupled (it holds no ActorRef back to
        // us). The cost is a brief window after a supervisor self-stops (idle
        // eviction) but before this fires, during which a direct table read
        // can still see `Ready` and hand out a handle to a stopping
        // supervisor; the caller's send then fails and surfaces as 503. If
        // draining ever grows long enough that this window causes user-visible
        // errors, give the supervisor an `ActorRef<CampaignRegistry>` and have
        // it mark `Draining` before `ctx.stop()` so reads reject cleanly.
        Ok(ControlFlow::Continue(()))
    }
}

/// Create a new campaign on this shard with the given owner. Idempotent
/// on `campaign_id`: if the supervisor already exists (or is loading),
/// returns its state rather than spawning a second one.
#[derive(Debug, Clone)]
pub struct CreateCampaign {
    pub campaign_id: CampaignId,
    pub owner_user_id: UserId,
}

impl Message<CreateCampaign> for CampaignRegistry {
    type Reply = Result<CampaignState, EnsureError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %msg.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: CreateCampaign,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let registry_ref = ctx.actor_ref().clone();
        self.ensure(registry_ref, msg.campaign_id, Some(msg.owner_user_id))
            .await
    }
}

/// Ensure a campaign is checked out on this shard. For cold checkouts of
/// existing campaigns (the DB already exists on disk or in object storage).
/// Does not set an owner; the campaign must already have one.
#[derive(Debug, Clone)]
pub struct EnsureCampaign {
    pub campaign_id: CampaignId,
}

impl Message<EnsureCampaign> for CampaignRegistry {
    type Reply = Result<CampaignState, EnsureError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %msg.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: EnsureCampaign,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let registry_ref = ctx.actor_ref().clone();
        self.ensure(registry_ref, msg.campaign_id, None).await
    }
}

/// Release a specific campaign from this shard. If the campaign is not
/// loaded, this is a no-op. Otherwise marks the entry `Draining`, tags the
/// supervisor with `PlatformRelease`, and stops it gracefully. The
/// `on_link_died` handler removes the entry once the supervisor finishes.
#[derive(Debug, Clone)]
pub struct ReleaseCampaign {
    pub campaign_id: CampaignId,
}

impl Message<ReleaseCampaign> for CampaignRegistry {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %msg.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: ReleaseCampaign,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let Some(state) = self.table.load().get(&msg.campaign_id).cloned() else {
            tracing::debug!("release requested for unloaded campaign; no-op");
            return;
        };
        if matches!(state, CampaignState::Draining { .. }) {
            tracing::debug!("release requested for already-draining campaign; no-op");
            return;
        }
        let supervisor = state.supervisor().clone();

        tracing::info!("releasing campaign (platform-initiated)");

        // Mark Draining so direct table reads 503 while it drains, instead of
        // handing out a handle to a stopping supervisor.
        self.write_table(|map| {
            map.insert(
                msg.campaign_id.clone(),
                CampaignState::Draining {
                    supervisor: supervisor.clone(),
                },
            );
        });

        let _ = supervisor
            .tell(SetStopCause(StopCause::PlatformRelease))
            .await;
        if let Err(e) = supervisor.stop_gracefully().await {
            tracing::warn!(error = ?e, "supervisor already stopping during release");
        }
    }
}

/// Reported by a load driver once a checkout reaches a terminal state. The
/// registry (sole table writer) flips `Loading -> Ready` or removes the entry.
struct LoadComplete {
    campaign_id: CampaignId,
    supervisor_id: ActorId,
    handle: Option<CampaignHandle>,
}

impl Message<LoadComplete> for CampaignRegistry {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: LoadComplete,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.write_table(|map| {
            // Guard the load-vs-death race: only act if the entry is still the
            // SAME `Loading` we started. If `on_link_died` already removed it
            // (supervisor went Ready then died instantly), drop this late
            // completion rather than resurrecting a dead supervisor as Ready.
            let still_ours = matches!(
                map.get(&msg.campaign_id),
                Some(CampaignState::Loading(e)) if e.supervisor.id() == msg.supervisor_id
            );
            if !still_ours {
                return;
            }
            match msg.handle {
                Some(handle) => {
                    map.insert(msg.campaign_id.clone(), CampaignState::Ready(handle));
                }
                None => {
                    map.remove(&msg.campaign_id);
                }
            }
        });
    }
}

impl CampaignRegistry {
    async fn ensure(
        &mut self,
        registry_ref: ActorRef<Self>,
        campaign_id: CampaignId,
        owner_user_id: Option<UserId>,
    ) -> Result<CampaignState, EnsureError> {
        if matches!(self.phase, Phase::Draining) {
            tracing::debug!("rejecting ensure during drain");
            return Err(EnsureError::ShuttingDown);
        }

        let current = self.table.load().get(&campaign_id).cloned();
        match current {
            Some(existing @ (CampaignState::Ready(_) | CampaignState::Loading(_))) => {
                tracing::debug!("campaign already checked out");
                Ok(existing)
            }
            Some(CampaignState::Draining { .. }) => {
                // Being torn down on this shard; the platform re-routes.
                tracing::debug!("rejecting ensure for draining campaign");
                Err(EnsureError::ShuttingDown)
            }
            None => {
                tracing::info!("starting async checkout");
                Ok(self
                    .start_load(registry_ref, campaign_id, owner_user_id)
                    .await)
            }
        }
    }

    /// Spawn the supervisor, link it, insert a `Loading` entry, and detach the
    /// driver. Returns the `Loading` state for the caller to await. Does NOT
    /// block on startup — that runs in the detached [`run_load`] task.
    async fn start_load(
        &self,
        registry_ref: ActorRef<Self>,
        campaign_id: CampaignId,
        owner_user_id: Option<UserId>,
    ) -> CampaignState {
        let supervisor = CampaignSupervisor::spawn(CampaignSupervisorArgs {
            campaign_id: campaign_id.clone(),
            owner_user_id,
            store: self.store.clone(),
            idle_timeout: self.idle_timeout,
            eviction_check_interval: self.eviction_check_interval,
            platform_client: self.platform_client.clone(),
        });

        // Link before the load drives so `on_link_died` is the authoritative
        // removal path even if the supervisor dies mid-startup. This is a fast
        // local registration, not a startup wait, so it's safe in the mailbox.
        registry_ref.link(&supervisor).await;

        let (tx, rx) = watch::channel(LoadOutcome::Pending);
        let state = CampaignState::Loading(LoadingEntry {
            rx,
            supervisor: supervisor.clone(),
        });
        self.write_table(|map| {
            map.insert(campaign_id.clone(), state.clone());
        });

        // Detached: a disconnecting client can't orphan a half-loaded
        // supervisor or wedge the entry in `Loading`.
        tokio::spawn(run_load(registry_ref, campaign_id, supervisor, tx));

        state
    }
}

/// Drive a checkout to a terminal state off the registry's mailbox: wait for
/// the supervisor's `on_start`, resolve its relationship graph, then publish
/// the outcome to awaiters (the `watch`) and to the registry ([`LoadComplete`],
/// the sole table writer).
#[tracing::instrument(skip_all, fields(campaign_id = %campaign_id.0))]
async fn run_load(
    registry_ref: ActorRef<CampaignRegistry>,
    campaign_id: CampaignId,
    supervisor: ActorRef<CampaignSupervisor>,
    tx: watch::Sender<LoadOutcome>,
) {
    let started = Instant::now();
    let supervisor_id = supervisor.id();

    supervisor.wait_for_startup().await;

    let handle = if !supervisor.is_alive() {
        tracing::warn!("supervisor died during startup");
        None
    } else {
        match supervisor.ask(RelationshipGraphRef).await {
            Ok(graph) => Some(CampaignHandle {
                supervisor: supervisor.clone(),
                graph,
            }),
            Err(e) => {
                tracing::warn!(error = %e, "supervisor unreachable resolving relationship graph");
                None
            }
        }
    };

    match handle {
        Some(handle) => {
            // Unblock awaiters first, then flip the table.
            let _ = tx.send(LoadOutcome::Ready(handle.clone()));
            let _ = registry_ref
                .tell(LoadComplete {
                    campaign_id,
                    supervisor_id,
                    handle: Some(handle),
                })
                .await;
            tracing::info!(
                init_total_elapsed_ms = started.elapsed().as_millis() as u64,
                "campaign loaded"
            );
        }
        None => {
            let _ = tx.send(LoadOutcome::Failed);
            let _ = registry_ref
                .tell(LoadComplete {
                    campaign_id,
                    supervisor_id,
                    handle: None,
                })
                .await;
        }
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

/// Returns the IDs of all campaigns currently in the routing table (any
/// state). Empty during drain (campaigns are being stopped, not available).
#[derive(Debug, Clone, Copy)]
pub struct ListLoaded;

impl Message<ListLoaded> for CampaignRegistry {
    type Reply = Vec<CampaignId>;

    async fn handle(
        &mut self,
        _: ListLoaded,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if matches!(self.phase, Phase::Draining) {
            return Vec::new();
        }
        self.table.load().keys().cloned().collect()
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

        // Snapshot supervisor refs and mark every entry `Draining` so direct
        // table reads 503 while children stop. run_drain only stops the
        // supervisors; their on_stop drains the graph + rooms in order, and
        // on_link_died removes each entry as it finishes.
        let snapshot = self.write_table(|map| {
            let snap: Vec<(CampaignId, ActorRef<CampaignSupervisor>)> = map
                .iter()
                .map(|(id, st)| (id.clone(), st.supervisor().clone()))
                .collect();
            for (id, sup) in &snap {
                map.insert(
                    id.clone(),
                    CampaignState::Draining {
                        supervisor: sup.clone(),
                    },
                );
            }
            snap
        });
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
