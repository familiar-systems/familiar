//! `ThingActor`: per-Thing CRDT room actor.
//!
//! Spawned by the `CampaignSupervisor` either lazily, when a client first
//! joins a Thing room (`ThingInit::Restore`, reconstructing a `LoroThingDoc`
//! from block rows in SQLite), or at creation time (`ThingInit::New`, where the
//! actor builds its doc and persists its own genesis row). Either way the actor
//! is the sole mutator of its Thing. Self-evicts when all subscribers leave and
//! an idle timer fires.

use std::time::Duration;

use chrono::Utc;
use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::{BlockId, ThingId};
use familiar_systems_campaign_shared::loro::thing::SECTION_CONTENT;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::actors::database_writer::{DatabaseWriteActor, DbCreateThing, WriteThingBlocks};
use crate::domain::crdt::doc::CrdtDoc;
use crate::domain::crdt::room;
use crate::domain::crdt::room_actor;
use crate::domain::thing::build_new_thing;
use crate::entities::columns::{BlockIdCol, StatusCol, ThingIdCol};
use crate::entities::{blocks, things};
use crate::loro::thing::LoroThingDoc;
use crate::wire::broadcast::encode_broadcast;
use crate::wire::fragmenter::BatchFragmenter;

pub struct ThingActor {
    campaign_id: CampaignId,
    thing_id: ThingId,
    doc_room: room::Room<LoroThingDoc>,
    db_writer: ActorRef<DatabaseWriteActor>,
    self_ref: ActorRef<ThingActor>,
    dirty: bool,
    debounce_duration: Duration,
    persist_timer: Option<tokio::task::JoinHandle<()>>,
    idle_timer: Option<tokio::task::JoinHandle<()>>,
    idle_timeout: Duration,
    fragmenter: BatchFragmenter,
}

pub struct ThingActorArgs {
    pub campaign_id: CampaignId,
    pub thing_id: ThingId,
    pub db_reader: DatabaseConnection,
    pub db_writer: ActorRef<DatabaseWriteActor>,
    /// Whether the actor loads an existing Thing or originates a new one.
    pub init: ThingInit,
    pub debounce_duration: Duration,
    pub idle_timeout: Duration,
}

/// How a `ThingActor` comes into being.
///
/// `Restore` loads an existing Thing's blocks from SQLite (the room-join path).
/// `New` originates a Thing: the actor builds its `LoroThingDoc` and persists
/// its own genesis row. Keeping creation inside the actor preserves the
/// invariant that every write to a Thing flows through its owning actor.
pub enum ThingInit {
    Restore,
    New { name: String, status: Status },
}

/// Failure modes for `ThingActor` startup.
#[derive(Debug, thiserror::Error)]
pub enum ThingInitError {
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("genesis write failed")]
    Genesis,
}

impl Actor for ThingActor {
    type Args = ThingActorArgs;
    type Error = ThingInitError;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %args.campaign_id.0, thing_id = %args.thing_id.0),
    )]
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let doc = match args.init {
            ThingInit::Restore => {
                let thing_row = things::Entity::find_by_id(ThingIdCol::from(args.thing_id.clone()))
                    .one(&args.db_reader)
                    .await
                    .inspect_err(|e| tracing::error!(error = %e, "failed to query thing"))?
                    .expect("ThingActor spawned for a thing that exists in the database");

                let block_rows = blocks::Entity::find()
                    .filter(blocks::Column::ThingId.eq(ThingIdCol::from(args.thing_id.clone())))
                    .filter(blocks::Column::Section.eq(SECTION_CONTENT))
                    .order_by_asc(blocks::Column::Ordering)
                    .all(&args.db_reader)
                    .await
                    .inspect_err(|e| tracing::error!(error = %e, "failed to query blocks"))?;

                let status: Status = thing_row.status.into();
                let blobs: Vec<Vec<u8>> = block_rows.into_iter().map(|b| b.content).collect();
                tracing::info!(block_count = blobs.len(), ?status, "thing actor restored");
                LoroThingDoc::from_blocks(&thing_row.name, &status, &blobs)
            }
            ThingInit::New { name, status } => {
                // The actor owns its own birth: build the doc, then persist the
                // genesis row through the single-writer. Nothing writes a
                // Thing's rows around the actor that owns it.
                let new_thing = build_new_thing(args.thing_id.clone(), name, status);
                let blobs: Vec<Vec<u8>> =
                    new_thing.blocks.iter().map(|b| b.content.clone()).collect();
                let doc = LoroThingDoc::from_blocks(&new_thing.name, &new_thing.status, &blobs);

                if let Err(e) = args.db_writer.ask(DbCreateThing { new_thing }).await {
                    tracing::error!(error = %e, "thing genesis write failed");
                    return Err(ThingInitError::Genesis);
                }
                tracing::info!(?status, "thing actor created");
                doc
            }
        };

        let doc_room = room::Room::new(doc);

        Ok(Self {
            campaign_id: args.campaign_id,
            thing_id: args.thing_id,
            doc_room,
            db_writer: args.db_writer,
            self_ref: actor_ref,
            dirty: false,
            debounce_duration: args.debounce_duration,
            persist_timer: None,
            idle_timer: None,
            idle_timeout: args.idle_timeout,
            fragmenter: BatchFragmenter::new(250 * 1024),
        })
    }

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0),
    )]
    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        tracing::debug!(?reason, dirty = self.dirty, "thing actor stopping");

        if self.dirty {
            if let Err(err) = self.persist_now().await {
                tracing::error!(error=%err, "failed to persist thing on stop");
            }
        } else {
            tracing::debug!("thing clean, no snapshot needed on stop");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CRDT Room
// ---------------------------------------------------------------------------

impl Message<room_actor::ClientJoin> for ThingActor {
    type Reply = Result<room_actor::JoinResponse, room_actor::JoinError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0, client_id = msg.client.0),
    )]
    async fn handle(
        &mut self,
        msg: room_actor::ClientJoin,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if let Some(handle) = self.idle_timer.take() {
            tracing::trace!("cancelling idle timer");
            handle.abort();
        }
        let capability = match msg.role {
            CampaignRole::Gm => room_actor::Capability::Write,
            CampaignRole::Player => room_actor::Capability::Read,
        };
        let response = self.doc_room.on_join(msg.client, msg.tx, capability);
        tracing::debug!(
            role = ?msg.role,
            ?capability,
            subscriber_count = self.doc_room.subscriber_count(),
            "client joined"
        );
        response
    }
}

impl Message<room_actor::ClientLeave> for ThingActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0, client_id = msg.client.0),
    )]
    async fn handle(
        &mut self,
        msg: room_actor::ClientLeave,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.doc_room.on_leave(msg.client);
        let subscriber_count = self.doc_room.subscriber_count();
        tracing::debug!(subscriber_count, "client left");
        if subscriber_count == 0 {
            tracing::trace!("last subscriber left, scheduling idle eviction");
            self.schedule_idle_eviction();
        }
    }
}

impl Message<room_actor::ClientUpdate> for ThingActor {
    type Reply = Result<room_actor::AckPayload, room_actor::UpdateError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0, client_id = msg.client.0),
    )]
    async fn handle(
        &mut self,
        msg: room_actor::ClientUpdate,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let total_bytes: usize = msg.updates.iter().map(Vec::len).sum();
        tracing::trace!(
            update_count = msg.updates.len(),
            total_bytes,
            "applying updates"
        );

        let old_version = self.doc_room.doc().version();
        let (broadcast, ack) = self
            .doc_room
            .apply_updates(msg.client, &msg.updates)
            .inspect_err(|e| tracing::warn!(error = ?e, "failed to apply client updates"))?;
        if old_version != ack.version {
            self.dirty = true;
            tracing::trace!(
                version_bytes = ack.version.0.len(),
                "doc advanced, scheduling persist"
            );
            self.schedule_persist();
        }
        let room_id = format!("thing:{}", self.thing_id.0);
        let frames = encode_broadcast(
            loro_protocol::CrdtType::Loro,
            &room_id,
            &broadcast.updates,
            &self.fragmenter,
        );
        tracing::trace!(frame_count = frames.len(), "fanning out broadcast");
        self.doc_room.fan_out(&frames, broadcast.exclude);
        Ok(ack)
    }
}

// ---------------------------------------------------------------------------
// PersistNow
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct PersistNow;

impl ThingActor {
    fn schedule_persist(&mut self) {
        tracing::trace!(debounce = ?self.debounce_duration, "scheduling persist");
        if let Some(handle) = self.persist_timer.take() {
            handle.abort();
        }
        let self_ref = self.self_ref.clone();
        let duration = self.debounce_duration;
        self.persist_timer = Some(tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            let _ = self_ref.tell(PersistNow).send().await;
        }));
    }

    fn schedule_idle_eviction(&mut self) {
        tracing::trace!(timeout = ?self.idle_timeout, "scheduling idle eviction");
        if let Some(handle) = self.idle_timer.take() {
            handle.abort();
        }
        let self_ref = self.self_ref.clone();
        let timeout = self.idle_timeout;
        self.idle_timer = Some(tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let _ = self_ref.tell(IdleEvict).send().await;
        }));
    }

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0),
    )]
    async fn persist_now(&mut self) -> Result<(), sea_orm::DbErr> {
        let extracted = self.doc_room.doc().extract_blocks();
        let title = self.doc_room.doc().read_title();
        let now = Utc::now();

        let block_rows: Vec<blocks::ActiveModel> = extracted
            .into_iter()
            .map(|b| blocks::ActiveModel {
                id: Set(BlockIdCol::from(BlockId::generate())),
                thing_id: Set(ThingIdCol::from(self.thing_id.clone())),
                status: Set(StatusCol::from(Status::GmOnly)),
                ordering: Set(b.ordering),
                content: Set(b.content),
                section: Set(SECTION_CONTENT.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            })
            .collect();

        let block_count = block_rows.len();
        tracing::debug!(block_count, "persisting thing blocks");

        if let Err(err) = self
            .db_writer
            .tell(WriteThingBlocks {
                thing_id: self.thing_id.clone(),
                blocks: block_rows,
                name_sync: title,
            })
            .await
        {
            tracing::error!(error=%err, "failed to send thing blocks to database");
        } else {
            tracing::debug!(block_count, "thing blocks enqueued for write");
        }

        self.dirty = false;
        Ok(())
    }
}

impl Message<PersistNow> for ThingActor {
    type Reply = Result<(), sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0),
    )]
    async fn handle(
        &mut self,
        _: PersistNow,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        tracing::trace!(dirty = self.dirty, "persist timer fired");
        if self.dirty {
            self.persist_now().await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// IdleEvict
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct IdleEvict;

impl Message<IdleEvict> for ThingActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0),
    )]
    async fn handle(&mut self, _: IdleEvict, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        let subscriber_count = self.doc_room.subscriber_count();
        if subscriber_count == 0 {
            tracing::info!("idle evicting thing actor");
            ctx.stop();
        } else {
            tracing::trace!(
                subscriber_count,
                "idle evict fired but subscribers present, skipping"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::migrations::Migrator;
    use familiar_systems_campaign_shared::loro::prosemirror::{
        ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
    };
    use kameo::actor::Spawn;
    use sea_orm::ActiveModelTrait;
    use sea_orm_migration::MigratorTrait;

    async fn setup_db() -> DatabaseConnection {
        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        conn
    }

    fn insert_thing(thing_id: &ThingId, name: &str) -> things::ActiveModel {
        let now = Utc::now();
        things::ActiveModel {
            id: Set(ThingIdCol::from(thing_id.clone())),
            name: Set(name.to_string()),
            status: Set(StatusCol::from(Status::GmOnly)),
            prototype_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
    }

    fn make_heading_blob(text: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            NODE_NAME_KEY: "heading",
            ATTRIBUTES_KEY: { "level": 1 },
            CHILDREN_KEY: [text]
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn starts_with_no_blocks() {
        let conn = setup_db().await;
        let thing_id = ThingId::generate();
        insert_thing(&thing_id, "Empty Thing")
            .insert(&conn)
            .await
            .unwrap();

        let campaign_id = CampaignId::generate();
        let db_writer =
            DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
                campaign_id: campaign_id.clone(),
                conn: conn.clone(),
            });

        let actor = ThingActor::spawn(ThingActorArgs {
            campaign_id,
            thing_id: thing_id.clone(),
            db_reader: conn,
            db_writer,
            init: ThingInit::Restore,
            debounce_duration: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(60),
        });
        actor.wait_for_startup().await;
        actor.stop_gracefully().await.unwrap();
        actor.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn new_init_persists_genesis_row() {
        let conn = setup_db().await;
        let campaign_id = CampaignId::generate();
        let db_writer =
            DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
                campaign_id: campaign_id.clone(),
                conn: conn.clone(),
            });

        let thing_id = ThingId::generate();
        let actor = ThingActor::spawn(ThingActorArgs {
            campaign_id,
            thing_id: thing_id.clone(),
            db_reader: conn.clone(),
            db_writer,
            init: ThingInit::New {
                name: "Korgath the Destroyer".into(),
                status: Status::GmOnly,
            },
            debounce_duration: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(60),
        });
        actor.wait_for_startup().await;
        assert!(actor.is_alive(), "genesis should succeed");

        // The actor wrote its own birth row.
        let row = things::Entity::find_by_id(ThingIdCol::from(thing_id.clone()))
            .one(&conn)
            .await
            .unwrap()
            .expect("genesis thing row exists");
        assert_eq!(row.name, "Korgath the Destroyer");
        assert_eq!(Status::from(row.status), Status::GmOnly);

        actor.stop_gracefully().await.unwrap();
        actor.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn restores_from_blocks() {
        let conn = setup_db().await;
        let thing_id = ThingId::generate();
        insert_thing(&thing_id, "Korgath")
            .insert(&conn)
            .await
            .unwrap();

        let now = Utc::now();
        blocks::ActiveModel {
            id: Set(BlockIdCol::from(BlockId::generate())),
            thing_id: Set(ThingIdCol::from(thing_id.clone())),
            status: Set(StatusCol::from(Status::GmOnly)),
            ordering: Set(0),
            content: Set(make_heading_blob("Korgath the Destroyer")),
            section: Set(SECTION_CONTENT.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .unwrap();

        let campaign_id = CampaignId::generate();
        let db_writer =
            DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
                campaign_id: campaign_id.clone(),
                conn: conn.clone(),
            });

        let actor = ThingActor::spawn(ThingActorArgs {
            campaign_id,
            thing_id,
            db_reader: conn,
            db_writer,
            init: ThingInit::Restore,
            debounce_duration: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(60),
        });
        actor.wait_for_startup().await;
        actor.stop_gracefully().await.unwrap();
        actor.wait_for_shutdown_with_result(|_| ()).await;
    }
}
