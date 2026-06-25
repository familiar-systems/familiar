//! WebSocket room dispatch: the `RoomHandle` the connection holds, the
//! `JoinRoom` resolution, and the lazy `ensure_page_actor` spawn.

use std::time::{Duration, Instant};

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_campaign_shared::id::{ClientId, PageId};
use kameo::actor::{ActorRef, Spawn};
use kameo::error::SendError;
use kameo::message::{Context, Message};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use tokio::sync::mpsc;

use super::CampaignSupervisor;
use crate::actors::page::{PageActor, PageActorArgs, PageInit};
use crate::actors::toc::TocActor;
use crate::domain::crdt::room_actor;
use crate::entities::columns::PageIdCol;
use crate::entities::pages;

// ---------------------------------------------------------------------------
// RoomHandle + JoinRoom (WebSocket room dispatch)
// ---------------------------------------------------------------------------

/// Handle to a room actor, held in the WebSocket connection's local routing
/// table. Enum (not trait object) because kameo `ActorRef<A>` is generic
/// over the concrete actor type.
#[derive(Clone)]
pub enum RoomHandle {
    Toc(ActorRef<TocActor>),
    Page(ActorRef<PageActor>),
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
            RoomHandle::Page(actor) => match actor.ask(msg).await {
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
            RoomHandle::Page(actor) => match actor.ask(msg).await {
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
            RoomHandle::Page(actor) => {
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
        fields(campaign_id = %self.campaign_id.0, page_id = %page_id.0),
    )]
    async fn ensure_page_actor(
        &mut self,
        page_id: PageId,
        supervisor_ref: ActorRef<Self>,
    ) -> Result<ActorRef<PageActor>, JoinRoomError> {
        if let Some(actor) = self.pages.get(&page_id)
            && actor.is_alive()
        {
            return Ok(actor.clone());
        }

        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");

        let exists = pages::Entity::find()
            .filter(pages::Column::Id.eq(PageIdCol::from(page_id.clone())))
            .count(db.reader())
            .await?
            > 0;

        if !exists {
            return Err(JoinRoomError::UnknownRoom(format!("page:{}", page_id.0)));
        }

        let actor = PageActor::spawn(PageActorArgs {
            campaign_id: self.campaign_id.clone(),
            page_id: page_id.clone(),
            db_reader: db.reader().clone(),
            db_writer: db.writer().clone(),
            toc: self.toc.clone(),
            init: PageInit::Restore,
            debounce_duration: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(30),
        });

        self.pages.insert(page_id, actor.clone());
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
            _ if msg.room_id.starts_with("page:") => {
                let id_str = &msg.room_id["page:".len()..];
                let ulid = ulid::Ulid::from_string(id_str)
                    .map_err(|_| JoinRoomError::UnknownRoom(msg.room_id.clone()))?;
                let page_id = PageId::from(ulid);
                let actor = self
                    .ensure_page_actor(page_id, ctx.actor_ref().clone())
                    .await?;
                Ok(RoomHandle::Page(actor))
            }
            _ => Err(JoinRoomError::UnknownRoom(msg.room_id)),
        }
    }
}
