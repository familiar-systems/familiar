//! `CampaignSupervisor`: per-campaign orchestrator.
//!
//! Owns the [`CampaignDatabase`] and an idle-eviction clock.
//!
//! Child actors:
//! - [`TocActor`] (CRDT singleton, eager)
//! - [`RelationshipGraph`] (server-authoritative graph singleton, eager)
//! - [`PageActor`] (per-page, lazy-spawned on first `JoinRoom`). Future: AgentConversation, CampaignVocabulary.
//!
//! Storage initialization (checkout, open connection, run migrations,
//! spawn DatabaseWriteActor) runs in `on_start` via
//! [`CampaignDatabase::checkout`]. The registry's `EnsureCampaign`
//! handler awaits the startup result before inserting the supervisor
//! into its map.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::{Duration, Instant};

use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::{ActorId, ActorRef, Spawn, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;

use super::{CreatePage, SetLandingPage};
use crate::actors::page::PageActor;
use crate::actors::relationship_graph::{RelationshipGraph, RelationshipGraphArgs};
use crate::actors::toc::{TocActor, TocActorArgs};
use crate::clients::platform_internal::PlatformInternalClient;
use crate::domain::page::DocumentPageKind;
use crate::error::InitError;
use crate::persistence::{CampaignDatabase, CampaignStore};
// TODO: replace `Option<CampaignDatabase>` with a `SupervisorState` enum
// (Starting / Restoring / Ready / Draining) per the actor domain design doc.
// The current Option works while checkout is synchronous and there are no
// room actors, but the state machine is needed for heartbeat phase reporting
// and room-join gating once WebSocket support lands.
pub struct CampaignSupervisor {
    pub(super) campaign_id: CampaignId,
    pub(super) store: Arc<dyn CampaignStore>,
    pub(super) db: Option<CampaignDatabase>,
    pub(super) toc: ActorRef<TocActor>,
    pub(super) relationship_graph: ActorRef<RelationshipGraph>,
    pub(super) pages: HashMap<PageId, ActorRef<PageActor>>,
    pub(super) last_activity: Instant,
    pub(super) idle_timeout: Duration,
    pub(super) stop_cause: Option<StopCause>,
    pub(super) platform_client: Option<PlatformInternalClient>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    Idle,
    Drain,
    RegistryFallback,
    PlatformRelease,
}

impl StopCause {
    fn as_str(self) -> &'static str {
        match self {
            StopCause::Idle => "idle",
            StopCause::Drain => "drain",
            StopCause::RegistryFallback => "registry_fallback",
            StopCause::PlatformRelease => "platform_release",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SetStopCause(pub StopCause);

impl Message<SetStopCause> for CampaignSupervisor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: SetStopCause,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.stop_cause.get_or_insert(msg.0);
    }
}

pub struct CampaignSupervisorArgs {
    pub campaign_id: CampaignId,
    pub owner_user_id: Option<UserId>,
    pub store: Arc<dyn CampaignStore>,
    pub idle_timeout: Duration,
    pub eviction_check_interval: Duration,
    pub platform_client: Option<PlatformInternalClient>,
}

impl Actor for CampaignSupervisor {
    type Args = CampaignSupervisorArgs;
    type Error = InitError;

    // TODO: move checkout to a background task per the actor domain design
    // doc's startup lifecycle. Synchronous on_start blocks the supervisor's
    // mailbox during checkout, which is fine for LocalCampaignStore (sub-ms)
    // but will block for seconds once S3 checkout downloads the .db file.
    // The design doc's pattern: spawn checkout as a tokio task, transition
    // through Starting -> Restoring -> Ready via completion messages.
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %args.campaign_id.0),
    )]
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, InitError> {
        let (db, is_new) = CampaignDatabase::checkout(
            args.store.as_ref(),
            &args.campaign_id,
            args.owner_user_id.as_ref(),
        )
        .await?;

        let toc = TocActor::spawn(TocActorArgs {
            campaign_id: args.campaign_id.clone(),
            db_reader: db.reader().clone(),
            db_writer: db.writer().clone(),
            debounce_duration: Duration::from_secs(2),
        });

        // TODO: supervise this child (kameo 0.20 `supervise(...).restart_policy(...)`)
        // instead of a bare `spawn`. Today a panic here is unobserved: the registry
        // caches this `ActorRef`, so a dead graph 503s every relationship op until the
        // campaign idle-evicts and re-checks-out. Supervised restart reuses the actor's
        // mailbox, so the cached ref stays valid across the restart - recovery without
        // taking the campaign down. Must reconcile with the manual `on_link_died` /
        // page-pruning path below.
        let relationship_graph = RelationshipGraph::spawn(RelationshipGraphArgs {
            campaign_id: args.campaign_id.clone(),
            db_reader: db.reader().clone(),
            db_writer: db.writer().clone(),
        });

        tracing::info!("campaign ready");

        let timer_ref = actor_ref.clone();
        let interval = args.eviction_check_interval;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            tick.tick().await;
            loop {
                tick.tick().await;
                if timer_ref.tell(IdleCheck).await.is_err() {
                    break;
                }
            }
        });

        // Seed the campaign's home page exactly once, on first-ever checkout.
        // Spawned (not inline) because on_start has not returned yet: the mailbox
        // is not draining, so a self-`ask` would deadlock. This runs after
        // on_start completes, going through the same CreatePage path the GM's
        // future "new page" button uses. Best-effort: a failure is cosmetic, and
        // the orphan path re-surfaces any created Page at the ToC root on the
        // next checkout.
        if is_new {
            let seed_ref = actor_ref.clone();
            tokio::spawn(async move {
                // The home page's sections (and the editable empty paragraph each
                // is seeded with, stable-id from genesis) come from its kind inside
                // the PageActor; this caller names the page and never enumerates
                // sections.
                match seed_ref
                    .ask(CreatePage {
                        name: "Campaign Base Camp".to_string(),
                        status: Some(Status::Known),
                        parent: None,
                        kind: DocumentPageKind::Entity,
                    })
                    .await
                {
                    // What happens if this fails to land correctly? Nothing, really.
                    // Users can still always make their own home pages and other new pages.
                    // Well, at least when setting a new home page lands - that's still a TODO!
                    Ok(page) => {
                        let page_id = PageId::from(page.id);
                        if let Err(e) = seed_ref.ask(SetLandingPage { page_id }).await {
                            tracing::warn!(error = %e, "failed to record campaign home page pointer");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to seed campaign home page");
                    }
                }
            });
        }

        Ok(Self {
            campaign_id: args.campaign_id,
            store: args.store,
            db: Some(db),
            toc,
            relationship_graph,
            pages: HashMap::new(),
            last_activity: Instant::now(),
            idle_timeout: args.idle_timeout,
            stop_cause: None,
            platform_client: args.platform_client,
        })
    }

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        let cause = match (self.stop_cause, &reason) {
            (Some(c), _) => c.as_str(),
            (None, ActorStopReason::Normal | ActorStopReason::SupervisorRestart) => "signal",
            (None, ActorStopReason::Killed) => "killed",
            (None, ActorStopReason::Panicked(_)) => "crash",
            (None, ActorStopReason::LinkDied { .. }) => "link_died",
        };
        tracing::info!(cause, "draining supervisor");
        let started = Instant::now();

        // Stop all PageActors, then TocActor, before DatabaseWriteActor
        // so any pending writebacks reach the writer before it drains.
        for (page_id, actor) in self.pages.drain() {
            if let Err(e) = actor.stop_gracefully().await {
                tracing::warn!(page_id = %page_id.0, error = ?e, "page actor already stopped during drain");
            }
            actor.wait_for_shutdown_with_result(|_| ()).await;
        }
        if let Err(e) = self.toc.stop_gracefully().await {
            tracing::warn!(error = ?e, "toc actor already stopped during drain");
        }
        self.toc.wait_for_shutdown_with_result(|_| ()).await;

        // The relationship graph writes through synchronously (no dirty state to
        // flush), but stop it before the DB connection closes for clean ordering.
        if let Err(e) = self.relationship_graph.stop_gracefully().await {
            tracing::warn!(error = ?e, "relationship graph already stopped during drain");
        }
        self.relationship_graph
            .wait_for_shutdown_with_result(|_| ())
            .await;

        if let Some(db) = self.db.take()
            && let Err(e) = db.release(self.store.as_ref(), &self.campaign_id).await
        {
            tracing::error!(
                error = %e,
                "storage release failed; campaign data may not be fully persisted"
            );
        }

        if matches!(self.stop_cause, Some(StopCause::Idle))
            && let Some(ref client) = self.platform_client
        {
            let campaign_id = self.campaign_id.0.0.clone();
            let client = client.clone();
            tokio::spawn(async move {
                if let Err(e) = client.release_lease(&campaign_id).await {
                    tracing::warn!(
                        campaign_id = %campaign_id,
                        error = %e,
                        "failed to notify platform of idle release"
                    );
                }
            });
        }

        tracing::info!(
            drain_elapsed_ms = started.elapsed().as_millis() as u64,
            "supervisor stopped"
        );
        Ok(())
    }

    // The supervisor is `link`ed to two kinds of siblings (kameo links are
    // bidirectional): its parent `CampaignRegistry` and each child `PageActor`
    // it spawns. This handler fires for both, so it must tell them apart and
    // react differently. Children are linked *after* insertion into `pages`, so
    // a dead sibling whose id is still in the map is necessarily a child.
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn on_link_died(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        id: ActorId,
        reason: ActorStopReason,
    ) -> Result<ControlFlow<ActorStopReason>, Self::Error> {
        // A child PageActor died (idle self-evict, drain, or panic). Prune its
        // entry and keep running: a single room going away (or even crashing)
        // must not take down the whole campaign and every other room. Each Page
        // is in `pages` when its one-shot link_died fires (linked after insert;
        // `on_stop`'s drain is terminal), so a hit here means "child".
        let before = self.pages.len();
        self.pages.retain(|_, actor| actor.id() != id);
        if self.pages.len() != before {
            tracing::debug!(
                ?reason,
                page_count = self.pages.len(),
                "page actor removed from supervisor via link_died"
            );
            return Ok(ControlFlow::Continue(()));
        }

        // No map entry matched: the only other sibling is the registry parent.
        // Preserve kameo's default propagation so an abnormal registry death
        // still tears the supervisor down (-> `on_stop` flush + DB release),
        // while a normal one is benign.
        match &reason {
            ActorStopReason::Normal | ActorStopReason::SupervisorRestart => {
                Ok(ControlFlow::Continue(()))
            }
            _ => Ok(ControlFlow::Break(ActorStopReason::LinkDied {
                id,
                reason: Box::new(reason),
            })),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IdleCheck;

impl Message<IdleCheck> for CampaignSupervisor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(&mut self, _: IdleCheck, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        if self.stop_cause.is_some() {
            return;
        }
        let elapsed = self.last_activity.elapsed();
        if elapsed >= self.idle_timeout {
            tracing::info!(
                idle_seconds = elapsed.as_secs(),
                "idle timeout reached, beginning eviction"
            );
            self.stop_cause = Some(StopCause::Idle);
            ctx.stop();
        } else {
            tracing::trace!(idle_seconds = elapsed.as_secs(), "idle check, still active");
        }
    }
}

// ---------------------------------------------------------------------------
// Ping (health check / test)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ping;

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub struct Pong;

impl Message<Ping> for CampaignSupervisor {
    type Reply = Pong;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.last_activity = Instant::now();
        Pong
    }
}

// ---------------------------------------------------------------------------
// RelationshipGraphRef (child-ref accessor)
// ---------------------------------------------------------------------------
//
// The supervisor is the campaign's lifecycle mailbox - the hottest path in the
// process - so relationship work must never serialize behind it. The registry asks
// this once at checkout (right after `wait_for_startup`) and caches the returned ref
// in its `CampaignHandle`; routes then talk to the `RelationshipGraph` directly and
// never touch the supervisor per request. A pure topology accessor - clone the eager
// singleton's ref, no `last_activity` bump - not a relationship operation. Future
// children (conversation actor, search actor) get a sibling accessor.

pub struct RelationshipGraphRef;

impl Message<RelationshipGraphRef> for CampaignSupervisor {
    type Reply = ActorRef<RelationshipGraph>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        _: RelationshipGraphRef,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.relationship_graph.clone()
    }
}
