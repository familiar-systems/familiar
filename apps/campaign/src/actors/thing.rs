//! `ThingActor`: per-Thing CRDT room actor.
//!
//! Spawned by the `CampaignSupervisor` either lazily, when a client first
//! joins a Thing room (`ThingInit::Restore`, reconstructing a `LoroThingDoc`
//! from block rows in SQLite), or at creation time (`ThingInit::New`, where the
//! actor builds its doc and persists its own genesis row). Either way the actor
//! is the sole mutator of its Thing. Born vacating, it self-evicts once it has
//! no subscribers and an idle timer fires, so a room that is never joined does
//! not leak resident until campaign drain.

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
use crate::actors::persist::{Persist, PersistError, PersistNow};
use crate::domain::crdt::doc::CrdtDoc;
use crate::domain::crdt::room;
use crate::domain::crdt::room_actor;
use crate::domain::thing::{NewBlock, build_new_thing};
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
    /// Whether the doc has unpersisted edits and, if so, the armed flush timer.
    /// See [`Persist`]; the timer is inseparable from dirtiness by construction.
    persist: Persist,
    debounce_duration: Duration,
    /// Whether the room has subscribers and, if not, the armed eviction timer.
    occupancy: Occupancy,
    idle_timeout: Duration,
    fragmenter: BatchFragmenter,
}

/// Whether a Thing room currently has subscribers.
///
/// `Occupied`: at least one client is connected; no eviction is scheduled.
/// `Vacating`: no clients are connected and an idle timer is counting down to
/// self-eviction. The timer is inseparable from vacancy, so "vacant but never
/// evicting" (a resident-actor leak) is unrepresentable. The subscriber set
/// itself lives in [`Room`](room::Room); this only tracks the eviction timer.
///
/// TODO: Consider moving to actors/occupancy.rs once conversations arrive.
enum Occupancy {
    Occupied,
    Vacating(tokio::task::JoinHandle<()>),
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
    New {
        name: String,
        status: Status,
        /// Initial content blocks: empty (`vec![]`) for a generic new Thing
        /// whose content arrives later through the editor, or one empty
        /// paragraph for the campaign home-page seed (so it opens editable).
        seed_blocks: Vec<NewBlock>,
    },
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
            ThingInit::New {
                name,
                status,
                seed_blocks,
            } => {
                // The actor owns its own birth: build the doc, then persist the
                // genesis row through the single-writer. Nothing writes a
                // Thing's rows around the actor that owns it.
                let new_thing = build_new_thing(args.thing_id.clone(), name, status, seed_blocks);
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

        // Born vacating: a freshly spawned room has no subscribers yet. The
        // imminent `ClientJoin` (room-join path) cancels the timer; if no join
        // ever arrives (e.g. a Thing created via the API but never opened), the
        // actor still self-evicts instead of leaking resident until drain.
        let mut the_self = Self {
            campaign_id: args.campaign_id,
            thing_id: args.thing_id,
            doc_room,
            db_writer: args.db_writer,
            self_ref: actor_ref,
            persist: Persist::new(),
            debounce_duration: args.debounce_duration,
            occupancy: Occupancy::Occupied,
            idle_timeout: args.idle_timeout,
            fragmenter: BatchFragmenter::new(250 * 1024),
        };
        the_self.schedule_idle_eviction();
        Ok(the_self)
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
        tracing::debug!(
            ?reason,
            dirty = self.persist.is_dirty(),
            "thing actor stopping"
        );

        // Last-ditch flush. We are stopping, so there is nothing to reschedule:
        // if this errors the recent edits are lost (logged). See `persist`'s
        // module docs on why eviction is not gated on persistence health.
        if self.persist.is_dirty() {
            if let Err(err) = self.flush().await {
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
        self.mark_occupied();
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
            tracing::trace!(
                version_bytes = ack.version.0.len(),
                "doc advanced, scheduling persist"
            );
            self.persist
                .schedule(&self.self_ref, self.debounce_duration);
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
// Persistence & occupancy timers
// ---------------------------------------------------------------------------

impl ThingActor {
    /// The room has a subscriber again: cancel any pending idle eviction.
    fn mark_occupied(&mut self) {
        if let Occupancy::Vacating(timer) = &self.occupancy {
            tracing::trace!("cancelling idle eviction");
            timer.abort();
        }
        self.occupancy = Occupancy::Occupied;
    }

    /// The room has no subscribers: arm idle self-eviction. Aborts any prior
    /// timer so only one is ever live.
    fn schedule_idle_eviction(&mut self) {
        tracing::trace!(timeout = ?self.idle_timeout, "scheduling idle eviction");
        if let Occupancy::Vacating(timer) = &self.occupancy {
            timer.abort();
        }
        let self_ref = self.self_ref.clone();
        let timeout = self.idle_timeout;
        self.occupancy = Occupancy::Vacating(tokio::spawn(async move {
            tokio::time::sleep(timeout).await;
            let _ = self_ref.tell(IdleEvict).send().await;
        }));
    }

    /// Serialize the doc to block rows and write them durably, awaiting the
    /// commit (`ask`, not `tell`). The error is returned so the caller keeps the
    /// actor dirty and retries; this never silently clears dirtiness.
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0),
    )]
    async fn flush(&mut self) -> Result<(), PersistError> {
        let extracted = self.doc_room.doc().extract_blocks();
        let title = self.doc_room.doc().read_title();
        let now = Utc::now();

        let block_rows: Vec<blocks::ActiveModel> = extracted
            .into_iter()
            .map(|b| {
                // Stable identity comes from the block's own `blockId` attribute
                // (assigned by the editor's unique-id extension, or at genesis).
                // Falling back to a fresh id only happens if it is missing or
                // malformed -- then the id churns across persists, which the
                // editor's unique-id plugin is there to prevent.
                let block_id = b.id.unwrap_or_else(|| {
                    tracing::warn!("persisted block has no stable blockId; minting a fresh one");
                    BlockId::generate()
                });
                blocks::ActiveModel {
                    id: Set(BlockIdCol::from(block_id)),
                    thing_id: Set(ThingIdCol::from(self.thing_id.clone())),
                    // Interim default: every block is GM-only until a per-block
                    // visibility control exists (no editor plugin sets status
                    // yet). Status is reset to gm_only on every persist.
                    status: Set(StatusCol::from(Status::GmOnly)),
                    ordering: Set(b.ordering),
                    content: Set(b.content),
                    section: Set(SECTION_CONTENT.to_string()),
                    created_at: Set(now),
                    updated_at: Set(now),
                }
            })
            .collect();

        let block_count = block_rows.len();
        tracing::debug!(block_count, "persisting thing blocks");

        self.db_writer
            .ask(WriteThingBlocks {
                thing_id: self.thing_id.clone(),
                blocks: block_rows,
                name_sync: title,
            })
            .await?;

        tracing::debug!(block_count, "thing blocks written");
        Ok(())
    }
}

impl Message<PersistNow> for ThingActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %self.thing_id.0),
    )]
    async fn handle(
        &mut self,
        _: PersistNow,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if !self.persist.is_dirty() {
            return;
        }
        let result = self.flush().await;
        self.persist
            .after_flush(result, &self.self_ref, self.debounce_duration);
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
                seed_blocks: vec![],
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

    /// A freshly spawned room is born vacating: with no client ever joining, the
    /// idle timer must still fire and self-evict, rather than leaking the actor
    /// resident until campaign drain.
    #[tokio::test]
    async fn born_vacating_self_evicts_without_a_join() {
        let conn = setup_db().await;
        let thing_id = ThingId::generate();
        insert_thing(&thing_id, "Unopened")
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
            idle_timeout: Duration::from_millis(40),
        });
        actor.wait_for_startup().await;

        // No client joins; the born-vacating idle timer should evict it.
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(
            !actor.is_alive(),
            "an un-joined thing self-evicts when idle"
        );
    }
}
