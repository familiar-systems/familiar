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
use std::sync::Arc;
use std::time::{Duration, Instant};

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{ActorRef, Spawn, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;

use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};

use familiar_systems_campaign_shared::id::{ClientId, ThingId};
use tokio::sync::mpsc;

use crate::actors::database_writer::{
    GetMetadata, MetadataError, PatchCampaignError, PatchCampaignMetadata as DbPatchCampaign,
    PatchCampaignResult,
};
use crate::actors::thing::{ThingActor, ThingActorArgs};
use crate::actors::toc::{TocActor, TocActorArgs};
use crate::clients::platform_internal::PlatformInternalClient;
use crate::domain::crdt::room_actor;
use crate::entities::columns::ThingIdCol;
use crate::entities::{campaign_metadata, things};
use crate::error::InitError;
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
        let db = CampaignDatabase::checkout(
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
        match self {
            RoomHandle::Toc(actor) => match actor.ask(msg).await {
                Ok(ack) => Ok(ack),
                Err(kameo::error::SendError::HandlerError(e)) => Err(e),
                Err(e) => Err(room_actor::UpdateError::Apply(e.to_string())),
            },
            RoomHandle::Thing(actor) => match actor.ask(msg).await {
                Ok(ack) => Ok(ack),
                Err(kameo::error::SendError::HandlerError(e)) => Err(e),
                Err(e) => Err(room_actor::UpdateError::Apply(e.to_string())),
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
            debounce_duration: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(30),
        });

        self.things.insert(thing_id, actor.clone());
        Ok(actor)
    }
}

impl Message<JoinRoom> for CampaignSupervisor {
    type Reply = Result<RoomHandle, JoinRoomError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, room_id = %msg.room_id),
    )]
    async fn handle(
        &mut self,
        msg: JoinRoom,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        match msg.room_id.as_str() {
            "toc" => Ok(RoomHandle::Toc(self.toc.clone())),
            _ if msg.room_id.starts_with("thing:") => {
                let id_str = &msg.room_id["thing:".len()..];
                let ulid = ulid::Ulid::from_string(id_str)
                    .map_err(|_| JoinRoomError::UnknownRoom(msg.room_id.clone()))?;
                let thing_id = ThingId::from(ulid);
                let actor = self.ensure_thing_actor(thing_id).await?;
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
}
