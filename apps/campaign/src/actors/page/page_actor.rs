//! `PageActor`: per-Page CRDT room actor.
//!
//! Spawned by the `CampaignSupervisor` either lazily, when a client first
//! joins a Page room (`PageInit::Restore`, reconstructing a `LoroPageDoc`
//! from block rows in SQLite), or at creation time (a `PageInit::New*` variant,
//! where the actor builds its doc and persists its own genesis row). Either way the actor
//! is the sole mutator of its Page. Born vacating, it self-evicts once it has
//! no subscribers and an idle timer fires, so a room that is never joined does
//! not leak resident until campaign drain.

use std::time::Duration;

use chrono::{DateTime, Utc};
use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::loro::page::Section;
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use sea_orm::ActiveValue::Set;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use tokio::sync::oneshot;

use crate::actors::database_writer::{
    CreatedSession, DatabaseWriteActor, DbCreatePage, DbCreateSession, WritePageBlocks,
};
use crate::actors::persist::{Persist, PersistError, PersistNow};
use crate::actors::toc::{TocActor, UpdatePageNode};
use crate::domain::crdt::doc::CrdtDoc;
use crate::domain::crdt::room;
use crate::domain::crdt::room_actor;
use crate::domain::page::{DocumentPageKind, NewBlock, NewPage, build_new_page};
use crate::entities::columns::{BlockIdCol, PageIdCol, SectionCol, StatusCol};
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

/// The Page's node-facing state, as last pushed to the ToC.
/// TODO add icons
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
/// The `New*` variants originate a Page: the actor builds its `LoroPageDoc` and
/// persists its own genesis row. Keeping creation inside the actor preserves the
/// invariant that every write to a Page flows through its owning actor.
///
/// `NewDocumentPage` is the **document-page** genesis path - `Entity` or
/// `Template`, structurally identical (`preamble` + `body`, persisted via
/// `DbCreatePage`), differing only in the [`DocumentPageKind`] stamped on the
/// `pages` row. A future Skill / Memory kind joins that sum. `NewSession` is the
/// odd one out: it mints a temporal `sessions` row alongside the page.
pub enum PageInit {
    Restore,
    /// Genesis a document page - `Entity` or `Template`, selected by
    /// [`DocumentPageKind`]. Both share the `preamble` + `body` layout and the
    /// `DbCreatePage` write; only the stamped `kind` differs, so an entity later
    /// cloned from a template inherits the same sections.
    NewDocumentPage {
        name: String,
        kind: DocumentPageKind,
        status: Status,
        /// Genesis hands the committed `pages::Model` back to the spawner (the
        /// supervisor) here, so it builds the HTTP response from the write edge
        /// rather than re-reading the row on the reader pool. The sender lives
        /// in the genesis variants, not on `PageActorArgs`: `Restore` produces
        /// no genesis payload, so an `Option<Sender>` there would be
        /// variant-specific state bolted onto a shared struct.
        reply: oneshot::Sender<pages::Model>,
    },
    /// Genesis a `Session` page together with its temporal `sessions` row, in one
    /// transaction via `DbCreateSession`. `name` is the page title (a neutral
    /// default when the GM gave none); the session's label lives on the page,
    /// the temporal row stays nameless.
    NewSession {
        name: String,
        status: Status,
        /// As `NewDocumentPage`, but the threaded payload also carries the
        /// temporal `sessions` row (the response needs its ordinal/created_at).
        reply: oneshot::Sender<CreatedSession>,
    },
}

/// Failure modes for `PageActor` startup.
#[derive(Debug, thiserror::Error)]
pub enum PageInitError {
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("genesis write failed")]
    Genesis,
}

/// Build the seeded `NewPage` for a genesis path and its matching `LoroPageDoc`.
///
/// The doc's section layout - and the empty paragraph seeded into each section
/// so it opens editable - comes from the `kind` inside `from_blocks`; the create
/// path passes no rows. We then read the seeded doc back into `new_page.blocks`
/// so the persisted rows are exactly what the doc contains, carrying the seeds'
/// stable blockIds (a block keeps one identity from genesis onward). Shared by
/// the entity (`NewEntity`), template (`NewTemplate`), and session
/// (`NewSession`) genesis arms; a future template-clone path slots in the same
/// way (its cloned blocks arriving as rows).
fn build_seeded_page(
    page_id: PageId,
    name: String,
    kind: PageKind,
    status: Status,
) -> (NewPage, LoroPageDoc) {
    let mut new_page = build_new_page(page_id, name, kind, status);
    let (doc, _) = LoroPageDoc::from_blocks(
        &new_page.name,
        &new_page.status,
        &new_page.kind,
        std::iter::empty::<(Section, Vec<u8>)>(),
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
    (new_page, doc)
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
                    block_rows
                        .into_iter()
                        .map(|b| (Section::from(b.section), b.content)),
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
            PageInit::NewDocumentPage {
                name,
                kind,
                status,
                reply,
            } => {
                // The actor owns its own birth: build the seeded doc, then persist
                // the genesis rows through the single-writer. Nothing writes a
                // Page's rows around the actor that owns it. Entity and Template
                // share this path (same `preamble` + `body` layout, same
                // `DbCreatePage` write); only the stamped `kind` differs, so an
                // entity later cloned from a template inherits the same sections.
                let (new_page, doc) =
                    build_seeded_page(args.page_id.clone(), name, kind.into(), status);
                match args.db_writer.ask(DbCreatePage { new_page }).await {
                    // Hand the committed row to the spawner. There is no fallible
                    // step after this in `on_start`, so a started actor always
                    // implies the send fired. A dropped receiver (the supervisor
                    // gave up) must not fail genesis, hence `let _`.
                    Ok(model) => {
                        let _ = reply.send(model);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "document page genesis write failed");
                        return Err(PageInitError::Genesis);
                    }
                }
                tracing::info!(?status, ?kind, "document page actor created");
                doc
            }
            PageInit::NewSession {
                name,
                status,
                reply,
            } => {
                // A session is born page-and-row together: the same seeded-doc
                // build as any page, but persisted through `DbCreateSession`,
                // which also mints the temporal `sessions` row (ordinal = max+1)
                // in the one genesis transaction. The page comes first so the
                // `sessions.page_id` FK resolves.
                let (new_page, doc) =
                    build_seeded_page(args.page_id.clone(), name, PageKind::Session, status);
                match args.db_writer.ask(DbCreateSession { new_page }).await {
                    Ok(created) => {
                        let _ = reply.send(created);
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "session genesis write failed");
                        return Err(PageInitError::Genesis);
                    }
                }
                tracing::info!(?status, "session page actor created");
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
                    section: Set(SectionCol::from(section)),
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
