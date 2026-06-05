//! `CampaignSupervisor`: per-campaign orchestrator.
//!
//! Owns the [`CampaignDatabase`] and an idle-eviction clock. Child room
//! actors: [`TocActor`] (singleton, eager), [`ThingActor`] (per-thing,
//! lazy-spawned on first `JoinRoom`). Future: AgentConversation,
//! RelationshipGraph, CampaignVocabulary.
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

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{ActorId, ActorRef, Spawn, WeakActorRef};
use kameo::error::{ActorStopReason, SendError};
use kameo::message::{Context, Message};
use kameo::prelude::Actor;

use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

use familiar_systems_campaign_shared::id::{BlockId, ClientId, ThingId};
use familiar_systems_campaign_shared::status::Status;
use tokio::sync::mpsc;

use crate::actors::database_writer::{
    DbSetLandingPage, GetMetadata, MetadataError, PatchCampaignError,
    PatchCampaignMetadata as DbPatchCampaign, PatchCampaignResult,
};
use crate::actors::thing::{ThingActor, ThingActorArgs, ThingInit};
use crate::actors::toc::{AddThingNode, ResolveThingNode, TocActor, TocActorArgs};
use crate::clients::platform_internal::PlatformInternalClient;
use crate::domain::crdt::room_actor;
use crate::domain::thing::NewBlock;
use crate::entities::columns::ThingIdCol;
use crate::entities::{campaign_metadata, things};
use crate::error::InitError;
use crate::loro::block_codec;
use crate::persistence::{CampaignDatabase, CampaignStore};

// TODO: replace `Option<CampaignDatabase>` with a `SupervisorState` enum
// (Starting / Restoring / Ready / Draining) per the actor domain design doc.
// The current Option works while checkout is synchronous and there are no
// room actors, but the state machine is needed for heartbeat phase reporting
// and room-join gating once WebSocket support lands.
pub struct CampaignSupervisor {
    campaign_id: CampaignId,
    store: Arc<dyn CampaignStore>,
    db: Option<CampaignDatabase>,
    toc: ActorRef<TocActor>,
    things: HashMap<ThingId, ActorRef<ThingActor>>,
    last_activity: Instant,
    idle_timeout: Duration,
    stop_cause: Option<StopCause>,
    platform_client: Option<PlatformInternalClient>,
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
        // on_start completes, going through the same CreateThing path the GM's
        // future "new page" button uses. Best-effort: a failure is cosmetic, and
        // the orphan path re-surfaces any created Thing at the ToC root on the
        // next checkout.
        if is_new {
            let seed_ref = actor_ref.clone();
            tokio::spawn(async move {
                // Seed one empty paragraph so the home page opens as a
                // schema-valid, editable document. Its ULID is embedded in the
                // block content (`attributes.blockId`) and used as the row id,
                // so the block keeps a stable identity from genesis onward.
                let block_id = BlockId::generate();
                let seed_blocks = vec![NewBlock {
                    id: block_id.clone(),
                    ordering: 0,
                    content: block_codec::empty_paragraph_blob(&block_id),
                    status: Status::GmOnly,
                }];
                match seed_ref
                    .ask(CreateThing {
                        name: "Campaign Base Camp".to_string(),
                        status: Some(Status::Known),
                        parent: None,
                        seed_blocks,
                    })
                    .await
                {
                    // What happens if this fails to land correctly? Nothing, really.
                    // Users can still always make their own home pages and other new pages.
                    // Well, at least when setting a new home page lands - that's still a TODO!
                    Ok(thing) => {
                        let thing_id = ThingId::from(thing.id);
                        if let Err(e) = seed_ref.ask(SetLandingPage { thing_id }).await {
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
            things: HashMap::new(),
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

        // Stop all ThingActors, then TocActor, before DatabaseWriteActor
        // so any pending writebacks reach the writer before it drains.
        for (thing_id, actor) in self.things.drain() {
            if let Err(e) = actor.stop_gracefully().await {
                tracing::warn!(thing_id = %thing_id.0, error = ?e, "thing actor already stopped during drain");
            }
            actor.wait_for_shutdown_with_result(|_| ()).await;
        }
        if let Err(e) = self.toc.stop_gracefully().await {
            tracing::warn!(error = ?e, "toc actor already stopped during drain");
        }
        self.toc.wait_for_shutdown_with_result(|_| ()).await;

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
    // bidirectional): its parent `CampaignRegistry` and each child `ThingActor`
    // it spawns. This handler fires for both, so it must tell them apart and
    // react differently. Children are linked *after* insertion into `things`, so
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
        // A child ThingActor died (idle self-evict, drain, or panic). Prune its
        // entry and keep running: a single room going away (or even crashing)
        // must not take down the whole campaign and every other room. Each Thing
        // is in `things` when its one-shot link_died fires (linked after insert;
        // `on_stop`'s drain is terminal), so a hit here means "child".
        let before = self.things.len();
        self.things.retain(|_, actor| actor.id() != id);
        if self.things.len() != before {
            tracing::debug!(
                ?reason,
                thing_count = self.things.len(),
                "thing actor removed from supervisor via link_died"
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
// PatchCampaignMetadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PatchCampaignMetadata {
    pub name: Option<String>,
    pub tagline: Option<String>,
    pub game_system: Option<String>,
    pub content_locale: Option<String>,
    pub complete_wizard: bool,
}

impl Message<PatchCampaignMetadata> for CampaignSupervisor {
    type Reply = Result<PatchCampaignResult, PatchCampaignError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: PatchCampaignMetadata,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        match db
            .writer()
            .ask(DbPatchCampaign {
                name: msg.name,
                tagline: msg.tagline,
                game_system: msg.game_system,
                content_locale: msg.content_locale,
                complete_wizard: msg.complete_wizard,
            })
            .await
        {
            Ok(result) => Ok(result),
            Err(kameo::error::SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "database actor unavailable during patch");
                Err(PatchCampaignError::ActorUnavailable)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GetMetadata
// ---------------------------------------------------------------------------

impl Message<GetMetadata> for CampaignSupervisor {
    type Reply = Result<campaign_metadata::Model, MetadataError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        _: GetMetadata,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(db.reader())
            .await?
            .ok_or(MetadataError::NoMetadataRow)
    }
}

// ---------------------------------------------------------------------------
// SetLandingPage
// ---------------------------------------------------------------------------

/// Point `campaign_metadata.home_thing_id` at a Thing (the campaign's home /
/// landing page). System-set during seeding, never mirrored to the platform
/// (it is a local display preference, unlike the wizard-seal metadata). Kept
/// distinct from `PatchCampaignMetadata` so the wizard path stays clean.
#[derive(Debug, Clone)]
pub struct SetLandingPage {
    pub thing_id: ThingId,
}

impl Message<SetLandingPage> for CampaignSupervisor {
    type Reply = Result<(), PatchCampaignError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %msg.thing_id.0),
    )]
    async fn handle(
        &mut self,
        msg: SetLandingPage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        match db
            .writer()
            .ask(DbSetLandingPage {
                thing_id: msg.thing_id,
            })
            .await
        {
            Ok(()) => Ok(()),
            Err(kameo::error::SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "database actor unavailable during set landing page");
                Err(PatchCampaignError::ActorUnavailable)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// CreateThing
// ---------------------------------------------------------------------------

/// Create a new Thing in this campaign. The supervisor validates placement,
/// spawns the owning `ThingActor` in genesis mode (which persists the Thing's
/// own birth row), registers it, and adds its node to the live ToC. Replies
/// with the persisted `things` row for the HTTP response.
#[derive(Debug, Clone)]
pub struct CreateThing {
    pub name: String,
    pub status: Option<Status>,
    /// Parent Thing to nest under in the ToC. `None` => ToC root.
    pub parent: Option<ThingId>,
    /// Initial content blocks. Empty for a generic new Thing; the home-page
    /// seed passes one empty paragraph so the page opens editable.
    pub seed_blocks: Vec<NewBlock>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateThingError {
    #[error("parent thing not found in toc")]
    ParentNotFound,
    #[error("thing genesis failed")]
    Genesis,
    #[error("a child actor was unavailable")]
    ActorUnavailable,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

impl Message<CreateThing> for CampaignSupervisor {
    type Reply = Result<things::Model, CreateThingError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: CreateThing,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();

        // Validate placement before any write: a bad parent fails cleanly with
        // nothing persisted.
        if let Some(parent) = &msg.parent {
            match self.toc.ask(ResolveThingNode(parent.clone())).await {
                Ok(Some(_)) => {}
                Ok(None) => return Err(CreateThingError::ParentNotFound),
                Err(e) => {
                    tracing::error!(error = %e, "toc unavailable while resolving parent");
                    return Err(CreateThingError::ActorUnavailable);
                }
            }
        }

        let status = msg.status.unwrap_or(Status::GmOnly);
        let thing_id = ThingId::generate();

        let (db_reader, db_writer) = {
            let db = self
                .db
                .as_ref()
                .expect("db must be Some while actor is running");
            (db.reader().clone(), db.writer().clone())
        };

        // Spawn the owning actor in genesis mode; it persists its own birth row
        // through the single-writer. Nothing writes a Thing's rows around it.
        let actor = ThingActor::spawn(ThingActorArgs {
            campaign_id: self.campaign_id.clone(),
            thing_id: thing_id.clone(),
            db_reader: db_reader.clone(),
            db_writer,
            init: ThingInit::New {
                name: msg.name.clone(),
                status,
                seed_blocks: msg.seed_blocks,
            },
            debounce_duration: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(30),
        });
        actor.wait_for_startup().await;
        if !actor.is_alive() {
            tracing::error!("thing actor died during genesis");
            return Err(CreateThingError::Genesis);
        }
        self.things.insert(thing_id.clone(), actor.clone());
        // Link after insert so `on_link_died` prunes this entry when the actor
        // self-evicts on idle (see the handler for the after-insert rationale).
        ctx.actor_ref().clone().link(&actor).await;

        // Place it in the live ToC. Best-effort: a failure here leaves a valid
        // Thing that `restore_toc` re-surfaces at the root on the next checkout.
        if let Err(e) = self
            .toc
            .ask(AddThingNode {
                thing_id: thing_id.clone(),
                title: msg.name.clone(),
                visibility: status,
                parent: msg.parent.clone(),
            })
            .await
        {
            tracing::error!(
                error = %e,
                "failed to add toc node for new thing; it will self-heal on next checkout"
            );
        }

        // Read back the committed row for the response.
        things::Entity::find_by_id(ThingIdCol::from(thing_id.clone()))
            .one(&db_reader)
            .await?
            .ok_or(CreateThingError::Genesis)
    }
}

// ---------------------------------------------------------------------------
// RoomHandle + JoinRoom (WebSocket room dispatch)
// ---------------------------------------------------------------------------

/// Handle to a room actor, held in the WebSocket connection's local routing
/// table. Enum (not trait object) because kameo `ActorRef<A>` is generic
/// over the concrete actor type.
#[derive(Clone)]
pub enum RoomHandle {
    Toc(ActorRef<TocActor>),
    Thing(ActorRef<ThingActor>),
}

impl RoomHandle {
    pub async fn join(
        &self,
        client: ClientId,
        tx: mpsc::UnboundedSender<Vec<u8>>,
        role: CampaignRole,
    ) -> Result<room_actor::JoinResponse, room_actor::JoinError> {
        let msg = room_actor::ClientJoin { client, tx, role };
        match self {
            RoomHandle::Toc(actor) => match actor.ask(msg).await {
                Ok(response) => Ok(response),
                Err(kameo::error::SendError::HandlerError(e)) => Err(e),
                Err(e) => Err(room_actor::JoinError::Internal(e.to_string())),
            },
            RoomHandle::Thing(actor) => match actor.ask(msg).await {
                Ok(response) => Ok(response),
                Err(kameo::error::SendError::HandlerError(e)) => Err(e),
                Err(e) => Err(room_actor::JoinError::Internal(e.to_string())),
            },
        }
    }

    pub async fn update(
        &self,
        client: ClientId,
        updates: Vec<Vec<u8>>,
    ) -> Result<room_actor::AckPayload, room_actor::UpdateError> {
        let msg = room_actor::ClientUpdate { client, updates };
        // Map kameo's transport-layer `SendError` to typed `UpdateError`
        // variants by matching the structured enum, not its Display text.
        // `ActorStopped`/`ActorNotRunning` mean the room actor is gone (a
        // self-evicted room is the common case); `MailboxFull`/`Timeout` mean
        // it is alive but overloaded. Mirrors the idiom in `actors/persist.rs`.
        match self {
            RoomHandle::Toc(actor) => match actor.ask(msg).await {
                Ok(ack) => Ok(ack),
                Err(SendError::HandlerError(e)) => Err(e),
                Err(SendError::ActorNotRunning(_) | SendError::ActorStopped) => {
                    Err(room_actor::UpdateError::RoomGone)
                }
                Err(SendError::MailboxFull(_) | SendError::Timeout(_)) => {
                    Err(room_actor::UpdateError::Busy)
                }
            },
            RoomHandle::Thing(actor) => match actor.ask(msg).await {
                Ok(ack) => Ok(ack),
                Err(SendError::HandlerError(e)) => Err(e),
                Err(SendError::ActorNotRunning(_) | SendError::ActorStopped) => {
                    Err(room_actor::UpdateError::RoomGone)
                }
                Err(SendError::MailboxFull(_) | SendError::Timeout(_)) => {
                    Err(room_actor::UpdateError::Busy)
                }
            },
        }
    }

    pub async fn leave(&self, client: ClientId) {
        let msg = room_actor::ClientLeave { client };
        match self {
            RoomHandle::Toc(actor) => {
                let _ = actor.tell(msg).await;
            }
            RoomHandle::Thing(actor) => {
                let _ = actor.tell(msg).await;
            }
        }
    }
}

pub struct JoinRoom {
    pub room_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum JoinRoomError {
    #[error("unknown room: {0}")]
    UnknownRoom(String),
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

impl CampaignSupervisor {
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %thing_id.0),
    )]
    async fn ensure_thing_actor(
        &mut self,
        thing_id: ThingId,
        supervisor_ref: ActorRef<Self>,
    ) -> Result<ActorRef<ThingActor>, JoinRoomError> {
        if let Some(actor) = self.things.get(&thing_id)
            && actor.is_alive()
        {
            return Ok(actor.clone());
        }

        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");

        let exists = things::Entity::find()
            .filter(things::Column::Id.eq(ThingIdCol::from(thing_id.clone())))
            .count(db.reader())
            .await?
            > 0;

        if !exists {
            return Err(JoinRoomError::UnknownRoom(format!("thing:{}", thing_id.0)));
        }

        let actor = ThingActor::spawn(ThingActorArgs {
            campaign_id: self.campaign_id.clone(),
            thing_id: thing_id.clone(),
            db_reader: db.reader().clone(),
            db_writer: db.writer().clone(),
            init: ThingInit::Restore,
            debounce_duration: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(30),
        });

        self.things.insert(thing_id, actor.clone());
        // Link after insert so `on_link_died` prunes this entry when the actor
        // self-evicts on idle (see the handler for the after-insert rationale).
        supervisor_ref.link(&actor).await;
        Ok(actor)
    }
}

impl Message<JoinRoom> for CampaignSupervisor {
    type Reply = Result<RoomHandle, JoinRoomError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, room_id = %msg.room_id),
    )]
    async fn handle(&mut self, msg: JoinRoom, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.last_activity = Instant::now();
        match msg.room_id.as_str() {
            "toc" => Ok(RoomHandle::Toc(self.toc.clone())),
            _ if msg.room_id.starts_with("thing:") => {
                let id_str = &msg.room_id["thing:".len()..];
                let ulid = ulid::Ulid::from_string(id_str)
                    .map_err(|_| JoinRoomError::UnknownRoom(msg.room_id.clone()))?;
                let thing_id = ThingId::from(ulid);
                let actor = self
                    .ensure_thing_actor(thing_id, ctx.actor_ref().clone())
                    .await?;
                Ok(RoomHandle::Thing(actor))
            }
            _ => Err(JoinRoomError::UnknownRoom(msg.room_id)),
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

/// Test-only probe for the private `things` map: is this Thing still tracked?
/// Lets eviction tests assert pruning without exposing the map.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ContainsThing(pub ThingId);

#[cfg(test)]
impl Message<ContainsThing> for CampaignSupervisor {
    type Reply = bool;

    async fn handle(
        &mut self,
        ContainsThing(id): ContainsThing,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.things.contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::register_sqlite_vec;
    use crate::persistence::LocalCampaignStore;
    use kameo::actor::Spawn;
    use tempfile::TempDir;

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

    /// A ThingActor that stops (idle self-eviction, or any stop) must be pruned
    /// from the supervisor's `things` map via the `link` + `on_link_died` edge,
    /// not left as a dead `ActorRef` until the next join of the same id.
    /// Drives the terminal effect directly with `stop_gracefully` (a `Normal`
    /// stop, same as `IdleEvict -> ctx.stop`) so the test is deterministic and
    /// needs no real idle wait.
    #[tokio::test]
    async fn evicted_thing_actor_is_pruned_from_map() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
        let supervisor = CampaignSupervisor::spawn(args);
        supervisor.wait_for_startup().await;

        // Create a Thing: it is inserted into `things` and linked.
        let model = supervisor
            .ask(CreateThing {
                name: "Ephemeral".to_string(),
                status: Some(Status::GmOnly),
                parent: None,
                seed_blocks: vec![],
            })
            .await
            .unwrap();
        let thing_id = ThingId::from(model.id.clone());
        assert!(
            supervisor
                .ask(ContainsThing(thing_id.clone()))
                .await
                .unwrap(),
            "newly created thing should be tracked",
        );

        // Join to obtain the live actor ref (returns the same in-map actor),
        // then stop it.
        let handle = supervisor
            .ask(JoinRoom {
                room_id: format!("thing:{}", thing_id.0),
            })
            .await
            .unwrap();
        let RoomHandle::Thing(actor) = handle else {
            panic!("expected a Thing room handle");
        };
        actor.stop_gracefully().await.unwrap();
        actor.wait_for_shutdown_with_result(|_| ()).await;

        // link_died is delivered after the actor terminates; poll until pruned.
        let mut pruned = false;
        for _ in 0..50 {
            if !supervisor
                .ask(ContainsThing(thing_id.clone()))
                .await
                .unwrap()
            {
                pruned = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            pruned,
            "dead thing actor should be pruned from the supervisor map"
        );

        supervisor.stop_gracefully().await.unwrap();
        supervisor.wait_for_shutdown_with_result(|_| ()).await;
    }

    /// Poll the campaign's SQLite file (the seed runs in a spawned task after
    /// `on_start`, so it lands asynchronously) until the home base exists.
    /// Asserts exactly one Thing named "Campaign Base Camp" (status `Known`)
    /// with `home_thing_id` pointing at it, and returns its id.
    async fn poll_until_seeded(db_path: &std::path::Path) -> ThingId {
        for _ in 0..200 {
            let conn = crate::db::connect_readonly(db_path)
                .await
                .expect("open readonly");
            let things = things::Entity::find()
                .all(&conn)
                .await
                .expect("query things");
            let meta = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
                .one(&conn)
                .await
                .expect("query metadata")
                .expect("metadata row exists");
            // Require both writes (the Thing row, then the pointer) so we never
            // observe the brief window between CreateThing and SetLandingPage.
            if let (Some(thing), Some(home)) = (things.first(), meta.home_thing_id.clone()) {
                assert_eq!(things.len(), 1, "exactly one Thing seeded");
                assert_eq!(thing.name, "Campaign Base Camp");
                assert_eq!(Status::from(thing.status), Status::Known);
                let thing_id = ThingId::from(thing.id.clone());
                assert_eq!(
                    ThingId::from(home),
                    thing_id,
                    "home_thing_id points at the base camp"
                );

                // The seed must give the home page exactly one block whose row
                // id equals the ULID embedded in its content (`attributes.blockId`).
                // This proves the page opens schema-valid (>=1 block) and that
                // block identity is stable, not minted fresh on each persist.
                let block_rows = crate::entities::blocks::Entity::find()
                    .filter(
                        crate::entities::blocks::Column::ThingId
                            .eq(ThingIdCol::from(thing_id.clone())),
                    )
                    .all(&conn)
                    .await
                    .expect("query blocks");
                assert_eq!(
                    block_rows.len(),
                    1,
                    "home page seeded with exactly one block"
                );
                let block = &block_rows[0];
                let row_id = BlockId::from(block.id.clone()).to_string();
                let content: serde_json::Value =
                    serde_json::from_slice(&block.content).expect("seed block content is JSON");
                assert_eq!(
                    content["attributes"]["blockId"].as_str(),
                    Some(row_id.as_str()),
                    "block row id equals the blockId embedded in its content",
                );
                assert_eq!(content["nodeName"].as_str(), Some("paragraph"));

                return thing_id;
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
        let things = things::Entity::find()
            .all(&conn)
            .await
            .expect("query things");
        assert_eq!(things.len(), 1, "reopen must not add a second base camp");
        let meta = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&conn)
            .await
            .expect("query metadata")
            .expect("metadata row exists");
        assert_eq!(
            meta.home_thing_id.map(ThingId::from),
            Some(seeded),
            "home pointer unchanged on reopen"
        );

        second.stop_gracefully().await.unwrap();
        second.wait_for_shutdown_with_result(|_| ()).await;
    }
}
