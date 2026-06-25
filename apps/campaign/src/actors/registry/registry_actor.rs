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
//! `run_drain`. The handler returns immediately, so the registry can
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

use crate::actors::relationship_graph::RelationshipGraph;
use crate::actors::supervisor::{
    CampaignSupervisor, CampaignSupervisorArgs, RelationshipGraphRef, SetStopCause, StopCause,
};
use crate::clients::platform_internal::PlatformInternalClient;
use crate::error::EnsureError;
use crate::persistence::CampaignStore;

pub const DRAIN_DEADLINE: Duration = Duration::from_secs(600);

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub enum Phase {
    Ready,
    Draining,
}

pub struct CampaignRegistry {
    supervisors: HashMap<CampaignId, CampaignHandle>,
    phase: Phase,
    store: Arc<dyn CampaignStore>,
    idle_timeout: Duration,
    eviction_check_interval: Duration,
    platform_client: Option<PlatformInternalClient>,
}

impl CampaignRegistry {
    pub fn new(
        store: Arc<dyn CampaignStore>,
        idle_timeout: Duration,
        eviction_check_interval: Duration,
        platform_client: Option<PlatformInternalClient>,
    ) -> Self {
        Self {
            supervisors: HashMap::new(),
            phase: Phase::Ready,
            store,
            idle_timeout,
            eviction_check_interval,
            platform_client,
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
            for (_id, handle) in self.supervisors.drain() {
                let sup_ref = handle.supervisor;
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
        self.supervisors
            .retain(|_, handle| handle.supervisor.id() != id);
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

/// Create a new campaign on this shard with the given owner. Idempotent
/// on `campaign_id`: if the supervisor already exists, returns it.
#[derive(Debug, Clone)]
pub struct CreateCampaign {
    pub campaign_id: CampaignId,
    pub owner_user_id: UserId,
}

impl Message<CreateCampaign> for CampaignRegistry {
    type Reply = Result<ActorRef<CampaignSupervisor>, EnsureError>;

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
        self.ensure_supervisor(registry_ref, msg.campaign_id, Some(msg.owner_user_id))
            .await
            .map(|handle| handle.supervisor)
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
    type Reply = Result<ActorRef<CampaignSupervisor>, EnsureError>;

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
        self.ensure_supervisor(registry_ref, msg.campaign_id, None)
            .await
            .map(|handle| handle.supervisor)
    }
}

/// Release a specific campaign from this shard. If the campaign is not
/// loaded, this is a no-op. If loaded, tags it with `PlatformRelease` and
/// stops it gracefully. The `on_link_died` handler removes the map entry
/// once the supervisor finishes shutting down.
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
        let Some(handle) = self.supervisors.get(&msg.campaign_id) else {
            tracing::debug!("release requested for unloaded campaign; no-op");
            return;
        };
        let supervisor = &handle.supervisor;

        tracing::info!("releasing campaign (platform-initiated)");

        let _ = supervisor
            .tell(SetStopCause(StopCause::PlatformRelease))
            .await;
        if let Err(e) = supervisor.stop_gracefully().await {
            tracing::warn!(error = ?e, "supervisor already stopping during release");
        }
    }
}

impl CampaignRegistry {
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %campaign_id.0),
    )]
    async fn ensure_supervisor(
        &mut self,
        registry_ref: ActorRef<Self>,
        campaign_id: CampaignId,
        owner_user_id: Option<UserId>,
    ) -> Result<CampaignHandle, EnsureError> {
        if matches!(self.phase, Phase::Draining) {
            tracing::debug!("rejecting ensure during drain");
            return Err(EnsureError::ShuttingDown);
        }

        if let Some(existing) = self.supervisors.get(&campaign_id) {
            tracing::debug!("supervisor already running");
            return Ok(existing.clone());
        }

        tracing::info!("spawning campaign supervisor");

        let started = Instant::now();

        let supervisor = CampaignSupervisor::spawn(CampaignSupervisorArgs {
            campaign_id: campaign_id.clone(),
            owner_user_id,
            store: self.store.clone(),
            idle_timeout: self.idle_timeout,
            eviction_check_interval: self.eviction_check_interval,
            platform_client: self.platform_client.clone(),
        });

        registry_ref.link(&supervisor).await;

        supervisor.wait_for_startup().await;
        if !supervisor.is_alive() {
            tracing::warn!("supervisor died during startup");
            return Err(EnsureError::SupervisorDied);
        }

        let graph = match supervisor.ask(RelationshipGraphRef).await {
            Ok(graph) => graph,
            Err(e) => {
                tracing::warn!(error = %e, "supervisor unreachable resolving relationship graph");
                return Err(EnsureError::SupervisorDied);
            }
        };

        let handle = CampaignHandle { supervisor, graph };
        self.supervisors.insert(campaign_id.clone(), handle.clone());

        tracing::info!(
            init_total_elapsed_ms = started.elapsed().as_millis() as u64,
            "campaign ensured"
        );
        Ok(handle)
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
    type Reply = Option<CampaignHandle>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %id.0),
    )]
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

/// Returns the IDs of all campaigns currently loaded in the registry.
/// Empty during drain (campaigns are being stopped, not available).
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
        self.supervisors.keys().cloned().collect()
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

        // Drain to bare supervisor refs - run_drain only stops supervisors; their
        // on_stop drains the graph + rooms in order.
        let snapshot: Vec<_> = self
            .supervisors
            .drain()
            .map(|(id, handle)| (id, handle.supervisor))
            .collect();
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
