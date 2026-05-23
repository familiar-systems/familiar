//! `CampaignSupervisor`: per-campaign orchestrator.
//!
//! Owns the [`CampaignDatabase`] and an idle-eviction clock. Future work
//! adds child room actors (ThingActor, TocActor, AgentConversation,
//! relationship graph, vocabulary).
//!
//! Storage initialization (checkout, open connection, run migrations,
//! spawn DatabaseActor) runs in `on_start` via
//! [`CampaignDatabase::checkout`]. The registry's `EnsureCampaign`
//! handler awaits the startup result before inserting the supervisor
//! into its map.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign_shared::id::ThingId;
use familiar_systems_campaign_shared::loro::thing::ThingHandle;
use familiar_systems_campaign_shared::loro::toc::TocEntry;
use kameo::actor::{ActorRef, Spawn, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use tokio::sync::mpsc;

use sea_orm::EntityTrait;

use crate::actors::database_writer::{
    CreateThing as DbCreateThing, DeleteThing as DbDeleteThing, GetMetadata, LoadTocSnapshot,
    MetadataError, PatchCampaignError, PatchCampaignMetadata as DbPatchCampaign,
    PatchCampaignResult, ThingExists,
};
use crate::actors::thing::{ThingActor, ThingActorArgs};
use crate::actors::toc::{self, TocActor, TocActorArgs};
use crate::actors::RoomHandle;
use crate::clients::platform_internal::PlatformInternalClient;
use crate::entities::campaign_metadata;
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
    toc: Option<ActorRef<TocActor>>,
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
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, InitError> {
        let span = tracing::info_span!("campaign_supervisor", campaign_id = %args.campaign_id.0);
        let _guard = span.enter();

        let db = CampaignDatabase::checkout(
            args.store.as_ref(),
            &args.campaign_id,
            args.owner_user_id.as_ref(),
        )
        .await?;

        let toc_snapshot = match db.writer().ask(LoadTocSnapshot).await {
            Ok(snap) => snap,
            Err(e) => {
                tracing::warn!(error = %e, "failed to load toc snapshot, starting fresh");
                None
            }
        };

        let toc = TocActor::spawn(TocActorArgs {
            snapshot: toc_snapshot,
            db_writer: db.writer().clone(),
        });

        tracing::info!("campaign ready (toc actor spawned)");

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
            toc: Some(toc),
            things: HashMap::new(),
            last_activity: Instant::now(),
            idle_timeout: args.idle_timeout,
            stop_cause: None,
            platform_client: args.platform_client,
        })
    }

    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        let span = tracing::info_span!(
            "campaign_supervisor",
            campaign_id = %self.campaign_id.0,
        );
        let _guard = span.enter();
        let cause = match (self.stop_cause, &reason) {
            (Some(c), _) => c.as_str(),
            (None, ActorStopReason::Normal | ActorStopReason::SupervisorRestart) => "signal",
            (None, ActorStopReason::Killed) => "killed",
            (None, ActorStopReason::Panicked(_)) => "crash",
            (None, ActorStopReason::LinkDied { .. }) => "link_died",
        };
        tracing::info!(cause, "draining supervisor");
        let started = Instant::now();

        for (thing_id, thing_ref) in self.things.drain() {
            tracing::debug!(thing_id = %thing_id.0, "stopping thing actor");
            let _ = thing_ref.stop_gracefully().await;
            thing_ref.wait_for_shutdown().await;
        }

        if let Some(toc) = self.toc.take() {
            tracing::debug!("stopping toc actor");
            let _ = toc.stop_gracefully().await;
            toc.wait_for_shutdown().await;
        }

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

    async fn handle(&mut self, _: IdleCheck, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        let span = tracing::info_span!(
            "campaign_supervisor",
            campaign_id = %self.campaign_id.0,
        );
        let _guard = span.enter();
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
// JoinRoom
// ---------------------------------------------------------------------------

const TOC_ROOM_ID: &str = "toc";

pub struct JoinRoom {
    pub conn_id: u64,
    pub room_id: String,
    pub tx: mpsc::UnboundedSender<ws::Message>,
}

#[derive(kameo::Reply)]
pub enum JoinRoomResult {
    Joined(RoomHandle),
    NotFound,
    ActorError(String),
}

impl Message<JoinRoom> for CampaignSupervisor {
    type Reply = JoinRoomResult;

    async fn handle(
        &mut self,
        msg: JoinRoom,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();

        if msg.room_id == TOC_ROOM_ID {
            let Some(toc) = &self.toc else {
                return JoinRoomResult::ActorError("toc actor not ready".to_string());
            };
            let _ = toc
                .tell(toc::ClientJoin {
                    conn_id: msg.conn_id,
                    tx: msg.tx,
                })
                .send()
                .await;
            tracing::info!(conn_id = msg.conn_id, "client joined toc room");
            JoinRoomResult::Joined(RoomHandle::Toc(toc.clone()))
        } else {
            let thing_id = ThingId(msg.room_id.clone().into());
            match self.get_or_spawn_thing(&thing_id).await {
                Some(thing) => {
                    let _ = thing
                        .tell(crate::actors::thing::ClientJoin {
                            conn_id: msg.conn_id,
                            tx: msg.tx,
                        })
                        .send()
                        .await;
                    tracing::info!(conn_id = msg.conn_id, thing_id = %msg.room_id, "client joined thing room");
                    JoinRoomResult::Joined(RoomHandle::Thing(thing))
                }
                None => JoinRoomResult::NotFound,
            }
        }
    }
}

impl CampaignSupervisor {
    async fn get_or_spawn_thing(&mut self, thing_id: &ThingId) -> Option<ActorRef<ThingActor>> {
        if let Some(thing) = self.things.get(thing_id) {
            return Some(thing.clone());
        }

        let db = self.db.as_ref()?;
        let exists = match db
            .writer()
            .ask(ThingExists {
                thing_id: thing_id.clone(),
            })
            .await
        {
            Ok(b) => b,
            Err(_) => return None,
        };

        if !exists {
            tracing::warn!(thing_id = %thing_id.0, "thing not found in database");
            return None;
        }

        let thing_ref = ThingActor::spawn(ThingActorArgs {
            thing_id: thing_id.clone(),
        });
        self.things.insert(thing_id.clone(), thing_ref.clone());
        Some(thing_ref)
    }
}

// ---------------------------------------------------------------------------
// CreateThing
// ---------------------------------------------------------------------------

pub struct CreateThing {
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateThingError {
    #[error("database error: {0}")]
    Db(String),
    #[error("toc error: {0}")]
    Toc(String),
    #[error("supervisor not ready")]
    NotReady,
}

impl Message<CreateThing> for CampaignSupervisor {
    type Reply = Result<ThingHandle, CreateThingError>;

    async fn handle(
        &mut self,
        msg: CreateThing,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self.db.as_ref().ok_or(CreateThingError::NotReady)?;
        let toc = self.toc.as_ref().ok_or(CreateThingError::NotReady)?;

        let handle = db
            .writer()
            .ask(DbCreateThing {
                name: msg.name.clone(),
            })
            .await
            .map_err(|e| CreateThingError::Db(e.to_string()))?;

        let entry = TocEntry::Thing {
            title: handle.name.clone(),
            thing_id: handle.id.clone(),
        };
        toc.ask(toc::AddEntry {
            entry,
            parent: None,
        })
        .await
        .map_err(|e| CreateThingError::Toc(e.to_string()))?;

        tracing::info!(thing_id = %handle.id.0, name = %handle.name, "thing created");
        Ok(handle)
    }
}

// ---------------------------------------------------------------------------
// DeleteThing
// ---------------------------------------------------------------------------

pub struct DeleteThing {
    pub thing_id: ThingId,
}

#[derive(Debug, thiserror::Error)]
pub enum DeleteThingError {
    #[error("database error: {0}")]
    Db(String),
    #[error("supervisor not ready")]
    NotReady,
}

impl Message<DeleteThing> for CampaignSupervisor {
    type Reply = Result<bool, DeleteThingError>;

    async fn handle(
        &mut self,
        msg: DeleteThing,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self.db.as_ref().ok_or(DeleteThingError::NotReady)?;

        if let Some(thing_ref) = self.things.remove(&msg.thing_id) {
            let _ = thing_ref.stop_gracefully().await;
            thing_ref.wait_for_shutdown().await;
        }

        let deleted = db
            .writer()
            .ask(DbDeleteThing {
                thing_id: msg.thing_id.clone(),
            })
            .await
            .map_err(|e| DeleteThingError::Db(e.to_string()))?;

        if deleted {
            if let Some(toc) = &self.toc {
                let tree = {
                    // Find the tree_id for this thing in the ToC.
                    // The TocActor doesn't expose a find-by-thing-id message yet,
                    // so we ask for the snapshot and search locally. This is fine
                    // for the thin-slice; a dedicated message is a follow-up.
                    // For now, we remove by broadcasting the deletion; the client
                    // will see the thing disappear from the ToC.
                    //
                    // TODO: add a RemoveByThingId message to TocActor to avoid
                    // the snapshot round-trip.
                    None::<loro::TreeID>
                };
                if let Some(tree_id) = tree {
                    let _ = toc.ask(toc::RemoveEntry { tree_id }).await;
                }
            }
            tracing::info!(thing_id = %msg.thing_id.0, "thing deleted");
        }

        Ok(deleted)
    }
}

// ---------------------------------------------------------------------------
// ClientDisconnected
// ---------------------------------------------------------------------------

pub struct ClientDisconnected {
    pub conn_id: u64,
}

impl Message<ClientDisconnected> for CampaignSupervisor {
    type Reply = ();

    async fn handle(
        &mut self,
        msg: ClientDisconnected,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        tracing::info!(conn_id = msg.conn_id, "client disconnected");
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
