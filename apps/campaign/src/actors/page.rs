//! `PageActor`: per-Page CRDT room actor.
//!
//! Spawned by the `CampaignSupervisor` either lazily, when a client first
//! joins a Page room (`PageInit::Restore`, reconstructing a `LoroPageDoc`
//! from block rows in SQLite), or at creation time (`PageInit::New`, where the
//! actor builds its doc and persists its own genesis row). Either way the actor
//! is the sole mutator of its Page. Born vacating, it self-evicts once it has
//! no subscribers and an idle timer fires, so a room that is never joined does
//! not leak resident until campaign drain.

use std::time::Duration;

use chrono::{DateTime, Utc};
use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};

use crate::actors::database_writer::{DatabaseWriteActor, DbCreatePage, WritePageBlocks};
use crate::actors::persist::{Persist, PersistError, PersistNow};
use crate::actors::toc::{TocActor, UpdatePageNode};
use crate::domain::crdt::doc::CrdtDoc;
use crate::domain::crdt::room;
use crate::domain::crdt::room_actor;
use crate::domain::page::{NewBlock, build_new_page};
use crate::entities::columns::{BlockIdCol, PageIdCol, StatusCol};
use crate::entities::{blocks, pages};
use crate::loro::page::LoroPageDoc;
use crate::wire::broadcast::encode_broadcast;
use crate::wire::fragmenter::BatchFragmenter;

pub struct PageActor {
    campaign_id: CampaignId,
    page_id: PageId,
    self_ref: ActorRef<PageActor>,

    // === CONTENT ===
    doc_room: room::Room<LoroPageDoc>,
    fragmenter: BatchFragmenter,
    // === PERSISTENCE ===
    debounce_duration: Duration,
    /// Whether the doc has unpersisted edits and, if so, the armed flush timer.
    persist: Persist,
    db_writer: ActorRef<DatabaseWriteActor>,
    // === TOC ===
    /// To send [`UpdatePageNode`] messages to the ToC on title and visibility change in real time.
    toc: ActorRef<TocActor>,
    /// Last thing pushed to the ToC. Skip if no changes, e.g. on body edit.
    /// Always populated (the title falls back to a recovery marker), so it never
    /// needs to be optional.
    last_node_state: NodeState,
    /// This actor incarnation's spawn time, carried with every ToC push. On
    /// evict+respawn the Page doc is rebuilt from storage and its version vector
    /// no longer descends from the pre-eviction one, so the vv alone cannot tell
    /// a fresh incarnation's edit from a stale in-flight push. `created_at`
    /// dominates the ordering; the vv only breaks ties within one incarnation.
    created_at: DateTime<Utc>,
    // === LIFETIME ===
    /// Whether the room has subscribers and, if not, the armed eviction timer.
    occupancy: Occupancy,
    idle_timeout: Duration,
}

/// Whether a Page room currently has subscribers.
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

/// The Page's node-facing state, as last pushed to the ToC. Named (rather than a
/// bare `(String, Status)` tuple) so the equality compare reads clearly and the
/// anticipated third field (icon) is a one-line add, not a `.0/.1/.2` churn.
#[derive(Clone, PartialEq, Eq)]
struct NodeState {
    title: String,
    visibility: Status,
}

pub struct PageActorArgs {
    pub campaign_id: CampaignId,
    pub page_id: PageId,
    pub db_reader: DatabaseConnection,
    pub db_writer: ActorRef<DatabaseWriteActor>,
    /// The campaign's ToC singleton, for server-authoritative node-state pushes.
    pub toc: ActorRef<TocActor>,
    /// Whether the actor loads an existing Page or originates a new one.
    pub init: PageInit,
    pub debounce_duration: Duration,
    pub idle_timeout: Duration,
}

/// How a `PageActor` comes into being.
///
/// `Restore` loads an existing Page's blocks from SQLite (the room-join path).
/// `New` originates a Page: the actor builds its `LoroPageDoc` and persists
/// its own genesis row. Keeping creation inside the actor preserves the
/// invariant that every write to a Page flows through its owning actor.
pub enum PageInit {
    Restore,
    New { name: String, status: Status },
}

/// Failure modes for `PageActor` startup.
#[derive(Debug, thiserror::Error)]
pub enum PageInitError {
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("genesis write failed")]
    Genesis,
}

impl Actor for PageActor {
    type Args = PageActorArgs;
    type Error = PageInitError;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %args.campaign_id.0, page_id = %args.page_id.0),
    )]
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let doc = match args.init {
            PageInit::Restore => {
                let page_row = pages::Entity::find_by_id(PageIdCol::from(args.page_id.clone()))
                    .one(&args.db_reader)
                    .await
                    .inspect_err(|e| tracing::error!(error = %e, "failed to query page"))?
                    .expect("PageActor spawned for a page that exists in the database");

                // All sections in one query, ordered by (section, ordering) so a
                // section's blocks arrive contiguous and in-order; `from_blocks`
                // buckets them into the kind's declared section containers.
                let block_rows = blocks::Entity::find()
                    .filter(blocks::Column::PageId.eq(PageIdCol::from(args.page_id.clone())))
                    .order_by_asc(blocks::Column::Section)
                    .order_by_asc(blocks::Column::Ordering)
                    .all(&args.db_reader)
                    .await
                    .inspect_err(|e| tracing::error!(error = %e, "failed to query blocks"))?;

                let status: Status = page_row.status.into();
                let kind: PageKind = page_row.kind.into();
                let block_count = block_rows.len();
                tracing::info!(block_count, ?status, ?kind, "page actor restored");
                let (doc, skipped) = LoroPageDoc::from_blocks(
                    &page_row.name,
                    &status,
                    &kind,
                    block_rows.into_iter().map(|b| (b.section, b.content)),
                    BlockId::generate,
                );
                // A dropped block means a corrupt blob slipped past the serialize
                // path -- it should never happen, so log the offending bytes in
                // full for triage. The Page still opens without it.
                for sb in &skipped {
                    tracing::error!(
                        ordering = sb.ordering,
                        blob = %String::from_utf8_lossy(&sb.blob),
                        error = %sb.reason,
                        "corrupt block dropped during page restore; this should never happen",
                    );
                }
                doc
            }
            PageInit::New { name, status } => {
                // The actor owns its own birth: build the doc, then persist the
                // genesis rows through the single-writer. Nothing writes a Page's
                // rows around the actor that owns it.
                //
                // The doc's section layout -- and the empty paragraph seeded into
                // each section so it opens editable -- comes from the kind, inside
                // `from_blocks`; the create path passes no sections. We then persist
                // exactly what the seeded doc contains, so the genesis rows carry
                // the seeds' stable blockIds (a block keeps one identity from
                // genesis onward) and the future template path slots in the same
                // way (its cloned blocks would arrive here as `rows`).
                let mut new_page = build_new_page(args.page_id.clone(), name, status);
                let (doc, _) = LoroPageDoc::from_blocks(
                    &new_page.name,
                    &new_page.status,
                    &new_page.kind,
                    std::iter::empty::<(String, Vec<u8>)>(),
                    BlockId::generate,
                );
                new_page.blocks = doc
                    .extract_sections()
                    .into_iter()
                    .flat_map(|(section, blocks)| {
                        blocks.into_iter().map(move |b| NewBlock {
                            id: b.id.unwrap_or_else(BlockId::generate),
                            section,
                            ordering: b.ordering,
                            content: b.content,
                            status: Status::GmOnly,
                        })
                    })
                    .collect();

                if let Err(e) = args.db_writer.ask(DbCreatePage { new_page }).await {
                    tracing::error!(error = %e, "page genesis write failed");
                    return Err(PageInitError::Genesis);
                }
                tracing::info!(?status, "page actor created");
                doc
            }
        };

        // The Page is the authority for its node-facing state; seed the
        // last-pushed snapshot from the freshly built doc so the first body-only
        // edit doesn't spuriously push to the ToC. The title resolves to a
        // recovery marker if `meta.title` is somehow empty (name-first means it
        // shouldn't be), so the seed is always populated.
        let initial_node_state = NodeState {
            title: doc.read_title_or_recovery_marker(&args.page_id),
            visibility: doc.read_status().unwrap_or(Status::GmOnly),
        };

        let doc_room = room::Room::new(doc);

        // Born vacating: a freshly spawned room has no subscribers yet. The
        // imminent `ClientJoin` (room-join path) cancels the timer; if no join
        // ever arrives (e.g. a Page created via the API but never opened), the
        // actor still self-evicts instead of leaking resident until drain.
        let mut the_self = Self {
            campaign_id: args.campaign_id,
            page_id: args.page_id,
            doc_room,
            db_writer: args.db_writer,
            toc: args.toc,
            self_ref: actor_ref,
            persist: Persist::new(),
            last_node_state: initial_node_state,
            created_at: Utc::now(),
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
        fields(campaign_id = %self.campaign_id.0, page_id = %self.page_id.0),
    )]
    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        tracing::debug!(
            ?reason,
            dirty = self.persist.is_dirty(),
            "page actor stopping"
        );

        // Last-ditch flush. We are stopping, so there is nothing to reschedule:
        // if this errors the recent edits are lost (logged). See `persist`'s
        // module docs on why eviction is not gated on persistence health.
        if self.persist.is_dirty() {
            if let Err(err) = self.flush().await {
                tracing::error!(error=%err, "failed to persist page on stop");
            }
        } else {
            tracing::debug!("page clean, no snapshot needed on stop");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CRDT Room
// ---------------------------------------------------------------------------

impl Message<room_actor::ClientJoin> for PageActor {
    type Reply = Result<room_actor::JoinResponse, room_actor::JoinError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %self.page_id.0, client_id = msg.client.0),
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

impl Message<room_actor::ClientLeave> for PageActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %self.page_id.0, client_id = msg.client.0),
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

impl Message<room_actor::ClientUpdate> for PageActor {
    type Reply = Result<room_actor::AckPayload, room_actor::UpdateError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %self.page_id.0, client_id = msg.client.0),
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
        let room_id = format!("page:{}", self.page_id.0);
        let frames = encode_broadcast(
            loro_protocol::CrdtType::Loro,
            &room_id,
            &broadcast.updates,
            &self.fragmenter,
        );
        tracing::trace!(frame_count = frames.len(), "fanning out broadcast");
        self.doc_room.fan_out(&frames, broadcast.exclude);

        // On title or visibility change, push to the ToC immediately.
        // If we have any failures, everything will self-heal on any of the following:
        // - The next checkout (the ToC is re-derived)
        // - A subsequent title change (the node state is updated)
        // - A subsequent visibility change (the node state is updated)
        // Always resolve a title (a recovery marker if `meta.title` is somehow
        // empty), so a visibility change is never gated on title presence and an
        // empty title surfaces loudly in the ToC instead of being silently kept.
        let title = self
            .doc_room
            .doc()
            .read_title_or_recovery_marker(&self.page_id);
        let visibility = self.doc_room.doc().read_status().unwrap_or(Status::GmOnly);
        let node_state = NodeState { title, visibility };
        if self.last_node_state != node_state {
            // Spawn so the edit path never blocks on the ToC. The spawned
            // `ask` reports any failure (transport or handler) in full; the
            // ToC node self-heals on checkout regardless.
            //
            // TODO(version-gating): spawning means these pushes can reach the
            // ToC out of order, and an evict+respawn resets the Page doc's vv
            // lineage, so pushes also cross incarnations. We carry `created_at`
            // (this incarnation's spawn time) and the Page doc's `version` so
            // the ToC can order them -- newer incarnation dominates, vv breaks
            // ties -- and drop stale ones (see the `UpdatePageNode` handler).
            // Until that is wired up we accept the race.
            let toc = self.toc.clone();
            let page_id = self.page_id.clone();
            let title = node_state.title.clone();
            let visibility = node_state.visibility;
            let version = self.doc_room.doc().version();
            let created_at = self.created_at;
            tokio::spawn(async move {
                let msg = UpdatePageNode {
                    page_id: page_id.clone(),
                    title,
                    visibility,
                    created_at,
                    version,
                };
                if let Err(err) = toc.ask(msg).await {
                    tracing::warn!(
                        error = %err,
                        page_id = %page_id.0,
                        "failed to push node state to toc",
                    );
                }
            });
            self.last_node_state = node_state;
        }

        Ok(ack)
    }
}

// ---------------------------------------------------------------------------
// Persistence & occupancy timers
// ---------------------------------------------------------------------------

impl PageActor {
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
        fields(campaign_id = %self.campaign_id.0, page_id = %self.page_id.0),
    )]
    async fn flush(&mut self) -> Result<(), PersistError> {
        let sections = self.doc_room.doc().extract_sections();
        let title = self.doc_room.doc().read_title();
        let now = Utc::now();
        let page_id_col = PageIdCol::from(self.page_id.clone());

        // One flat row set across all sections. `ordering` is per-section (each
        // container indexes its children from 0), so blocks sort within their
        // section, not globally; `section` carries which container they belong to.
        let mut block_rows: Vec<blocks::ActiveModel> = Vec::new();
        for (section, blocks) in sections {
            for b in blocks {
                // Stable identity comes from the block's own `blockId` attribute
                // (assigned by the editor's unique-id extension, or at genesis).
                // Falling back to a fresh id only happens if it is missing or
                // malformed -- then the id churns across persists, which the
                // editor's unique-id plugin is there to prevent.
                let block_id = b.id.unwrap_or_else(|| {
                    tracing::warn!("persisted block has no stable blockId; minting a fresh one");
                    BlockId::generate()
                });
                block_rows.push(blocks::ActiveModel {
                    id: Set(BlockIdCol::from(block_id)),
                    page_id: Set(page_id_col.clone()),
                    // Interim default: every block is GM-only until a per-block
                    // visibility control exists (no editor plugin sets status
                    // yet). Status is reset to gm_only on every persist.
                    status: Set(StatusCol::from(Status::GmOnly)),
                    ordering: Set(b.ordering),
                    content: Set(b.content),
                    section: Set(section.to_string()),
                    created_at: Set(now),
                    updated_at: Set(now),
                });
            }
        }

        let block_count = block_rows.len();
        tracing::debug!(block_count, "persisting page blocks");

        self.db_writer
            .ask(WritePageBlocks {
                page_id: self.page_id.clone(),
                blocks: block_rows,
                name_sync: title,
            })
            .await?;

        tracing::debug!(block_count, "page blocks written");
        Ok(())
    }
}

impl Message<PersistNow> for PageActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %self.page_id.0),
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

impl Message<IdleEvict> for PageActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %self.page_id.0),
    )]
    async fn handle(&mut self, _: IdleEvict, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        let subscriber_count = self.doc_room.subscriber_count();
        if subscriber_count == 0 {
            tracing::info!("idle evicting page actor");
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
    use crate::actors::toc::{AddPageNode, ReadPageNodeTitle, TocActorArgs};
    use crate::db;
    use crate::entities::columns::PageKindCol;
    use crate::migrations::Migrator;
    use familiar_systems_campaign_shared::loro::page::SECTION_BODY;
    use familiar_systems_campaign_shared::loro::prosemirror::{
        ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
    };
    use kameo::actor::Spawn;
    use sea_orm::ActiveModelTrait;
    use sea_orm_migration::MigratorTrait;

    /// Spawn a ToC actor for tests that need a `PageActor`'s required `toc` ref.
    /// Borrows its deps so the caller can still move `conn`/`db_writer`/
    /// `campaign_id` into `PageActorArgs` afterward.
    fn spawn_toc(
        campaign_id: &CampaignId,
        conn: &DatabaseConnection,
        db_writer: &ActorRef<DatabaseWriteActor>,
    ) -> ActorRef<TocActor> {
        TocActor::spawn(TocActorArgs {
            campaign_id: campaign_id.clone(),
            db_reader: conn.clone(),
            db_writer: db_writer.clone(),
            debounce_duration: Duration::from_secs(60),
        })
    }

    async fn setup_db() -> DatabaseConnection {
        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        conn
    }

    fn insert_page(page_id: &PageId, name: &str) -> pages::ActiveModel {
        let now = Utc::now();
        pages::ActiveModel {
            id: Set(PageIdCol::from(page_id.clone())),
            name: Set(name.to_string()),
            status: Set(StatusCol::from(Status::GmOnly)),
            kind: Set(PageKindCol::from(PageKind::Entity)),
            template_id: Set(None),
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
        let page_id = PageId::generate();
        insert_page(&page_id, "Empty Page")
            .insert(&conn)
            .await
            .unwrap();

        let campaign_id = CampaignId::generate();
        let db_writer =
            DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
                campaign_id: campaign_id.clone(),
                conn: conn.clone(),
            });

        let toc = spawn_toc(&campaign_id, &conn, &db_writer);
        let actor = PageActor::spawn(PageActorArgs {
            campaign_id,
            page_id: page_id.clone(),
            db_reader: conn,
            db_writer,
            toc,
            init: PageInit::Restore,
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

        let page_id = PageId::generate();
        let toc = spawn_toc(&campaign_id, &conn, &db_writer);
        let actor = PageActor::spawn(PageActorArgs {
            campaign_id,
            page_id: page_id.clone(),
            db_reader: conn.clone(),
            db_writer,
            toc,
            init: PageInit::New {
                name: "Korgath the Destroyer".into(),
                status: Status::GmOnly,
            },
            debounce_duration: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(60),
        });
        actor.wait_for_startup().await;
        assert!(actor.is_alive(), "genesis should succeed");

        // The actor wrote its own birth row.
        let row = pages::Entity::find_by_id(PageIdCol::from(page_id.clone()))
            .one(&conn)
            .await
            .unwrap()
            .expect("genesis page row exists");
        assert_eq!(row.name, "Korgath the Destroyer");
        assert_eq!(Status::from(row.status), Status::GmOnly);

        actor.stop_gracefully().await.unwrap();
        actor.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn restores_from_blocks() {
        let conn = setup_db().await;
        let page_id = PageId::generate();
        insert_page(&page_id, "Korgath")
            .insert(&conn)
            .await
            .unwrap();

        let now = Utc::now();
        blocks::ActiveModel {
            id: Set(BlockIdCol::from(BlockId::generate())),
            page_id: Set(PageIdCol::from(page_id.clone())),
            status: Set(StatusCol::from(Status::GmOnly)),
            ordering: Set(0),
            content: Set(make_heading_blob("Korgath the Destroyer")),
            section: Set(SECTION_BODY.to_string()),
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

        let toc = spawn_toc(&campaign_id, &conn, &db_writer);
        let actor = PageActor::spawn(PageActorArgs {
            campaign_id,
            page_id,
            db_reader: conn,
            db_writer,
            toc,
            init: PageInit::Restore,
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
        let page_id = PageId::generate();
        insert_page(&page_id, "Unopened")
            .insert(&conn)
            .await
            .unwrap();

        let campaign_id = CampaignId::generate();
        let db_writer =
            DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
                campaign_id: campaign_id.clone(),
                conn: conn.clone(),
            });

        let toc = spawn_toc(&campaign_id, &conn, &db_writer);
        let actor = PageActor::spawn(PageActorArgs {
            campaign_id,
            page_id,
            db_reader: conn,
            db_writer,
            toc,
            init: PageInit::Restore,
            debounce_duration: Duration::from_secs(60),
            idle_timeout: Duration::from_millis(40),
        });
        actor.wait_for_startup().await;

        // No client joins; the born-vacating idle timer should evict it.
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert!(!actor.is_alive(), "an un-joined page self-evicts when idle");
    }

    /// The server-authoritative title path, end to end: a GM joins a Page room,
    /// edits `meta.title` over a `ClientUpdate`, and the owning PageActor pushes
    /// the rename to the wired TocActor so the live ToC node reflects it.
    #[tokio::test]
    async fn meta_title_edit_pushes_rename_to_toc() {
        use familiar_systems_campaign_shared::id::ClientId;
        use familiar_systems_campaign_shared::loro::page::{CONTAINER_META, KEY_TITLE};
        use loro::LoroDoc;
        use std::borrow::Cow;

        let conn = setup_db().await;
        let campaign_id = CampaignId::generate();
        let db_writer =
            DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
                campaign_id: campaign_id.clone(),
                conn: conn.clone(),
            });
        let toc = TocActor::spawn(TocActorArgs {
            campaign_id: campaign_id.clone(),
            db_reader: conn.clone(),
            db_writer: db_writer.clone(),
            debounce_duration: Duration::from_secs(60),
        });
        toc.wait_for_startup().await;

        // Genesis a Page named "Original" (writes its own row), wired to the toc.
        let page_id = PageId::generate();
        let page = PageActor::spawn(PageActorArgs {
            campaign_id: campaign_id.clone(),
            page_id: page_id.clone(),
            db_reader: conn.clone(),
            db_writer,
            toc: toc.clone(),
            init: PageInit::New {
                name: "Original".into(),
                status: Status::GmOnly,
            },
            debounce_duration: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(60),
        });
        page.wait_for_startup().await;

        // Mirror the supervisor: place the page's node in the ToC.
        toc.ask(AddPageNode {
            page_id: page_id.clone(),
            title: "Original".into(),
            visibility: Status::GmOnly,
            parent: None,
        })
        .await
        .expect("add page node");
        assert_eq!(
            toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap(),
            Some("Original".to_string()),
        );

        // Join as GM (Write capability) and take the server snapshot.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let client = ClientId::new(1);
        let join = page
            .ask(room_actor::ClientJoin {
                client,
                tx,
                role: CampaignRole::Gm,
            })
            .await
            .expect("join");

        // Build a client doc from the snapshot, rename via meta.title, and ship
        // the delta back as a ClientUpdate (the real edit path).
        let client_doc = LoroDoc::new();
        client_doc.import(join.snapshot.as_bytes()).unwrap();
        let before = client_doc.oplog_vv();
        client_doc
            .get_map(CONTAINER_META)
            .insert(KEY_TITLE, "Renamed")
            .unwrap();
        client_doc.commit();
        let update = client_doc
            .export(loro::ExportMode::Updates {
                from: Cow::Owned(before),
            })
            .unwrap();

        page.ask(room_actor::ClientUpdate {
            client,
            updates: vec![update],
        })
        .await
        .expect("apply update");

        // The PageActor spawns the push to the TocActor, so the rename lands
        // asynchronously after the ClientUpdate reply returns; poll until it does.
        let mut renamed = false;
        for _ in 0..50 {
            if toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap()
                == Some("Renamed".to_string())
            {
                renamed = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            renamed,
            "PageActor must push the rename to the live ToC node"
        );

        page.stop_gracefully().await.unwrap();
        page.wait_for_shutdown_with_result(|_| ()).await;
        toc.stop_gracefully().await.unwrap();
        toc.wait_for_shutdown_with_result(|_| ()).await;
    }

    /// An empty `meta.title` should never happen (Pages are name-first), but if
    /// a client commits one it must surface loudly, not silently keep the stale
    /// title. Clearing the title pushes the deterministic recovery marker to the
    /// live ToC node; the old `if let Some(title)` gate would have dropped it.
    #[tokio::test]
    async fn clearing_title_pushes_recovery_marker_to_toc() {
        use familiar_systems_campaign_shared::id::ClientId;
        use familiar_systems_campaign_shared::loro::page::{CONTAINER_META, KEY_TITLE};
        use loro::LoroDoc;
        use std::borrow::Cow;

        let conn = setup_db().await;
        let campaign_id = CampaignId::generate();
        let db_writer =
            DatabaseWriteActor::spawn(crate::actors::database_writer::DatabaseWriteActorArgs {
                campaign_id: campaign_id.clone(),
                conn: conn.clone(),
            });
        let toc = TocActor::spawn(TocActorArgs {
            campaign_id: campaign_id.clone(),
            db_reader: conn.clone(),
            db_writer: db_writer.clone(),
            debounce_duration: Duration::from_secs(60),
        });
        toc.wait_for_startup().await;

        // Genesis a Page named "Original" (writes its own row), wired to the toc.
        let page_id = PageId::generate();
        let page = PageActor::spawn(PageActorArgs {
            campaign_id: campaign_id.clone(),
            page_id: page_id.clone(),
            db_reader: conn.clone(),
            db_writer,
            toc: toc.clone(),
            init: PageInit::New {
                name: "Original".into(),
                status: Status::GmOnly,
            },
            debounce_duration: Duration::from_secs(60),
            idle_timeout: Duration::from_secs(60),
        });
        page.wait_for_startup().await;

        // Mirror the supervisor: place the page's node in the ToC.
        toc.ask(AddPageNode {
            page_id: page_id.clone(),
            title: "Original".into(),
            visibility: Status::GmOnly,
            parent: None,
        })
        .await
        .expect("add page node");

        // Join as GM, then clear meta.title to "" over a ClientUpdate.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let client = ClientId::new(1);
        let join = page
            .ask(room_actor::ClientJoin {
                client,
                tx,
                role: CampaignRole::Gm,
            })
            .await
            .expect("join");

        let client_doc = LoroDoc::new();
        client_doc.import(join.snapshot.as_bytes()).unwrap();
        let before = client_doc.oplog_vv();
        client_doc
            .get_map(CONTAINER_META)
            .insert(KEY_TITLE, "")
            .unwrap();
        client_doc.commit();
        let update = client_doc
            .export(loro::ExportMode::Updates {
                from: Cow::Owned(before),
            })
            .unwrap();

        page.ask(room_actor::ClientUpdate {
            client,
            updates: vec![update],
        })
        .await
        .expect("apply update");

        // The push lands asynchronously; poll until the ToC node carries the
        // recovery marker (the page's id makes it identifiable).
        let marker = format!("ERROR LOADING TITLE {}", page_id.0);
        let mut surfaced = false;
        for _ in 0..50 {
            if toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap() == Some(marker.clone()) {
                surfaced = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            surfaced,
            "clearing the title must push the recovery marker to the live ToC node"
        );

        page.stop_gracefully().await.unwrap();
        page.wait_for_shutdown_with_result(|_| ()).await;
        toc.stop_gracefully().await.unwrap();
        toc.wait_for_shutdown_with_result(|_| ()).await;
    }
}
