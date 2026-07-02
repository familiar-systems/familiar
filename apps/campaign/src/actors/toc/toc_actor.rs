use chrono::{DateTime, Utc};
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::loro::toc::{TocEntry, TocPageKind};
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use loro::TreeID;
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::actors::database_writer::{DatabaseWriteActor, WriteTocSnapshot};
use crate::actors::persist::{Persist, PersistError, PersistNow};
use crate::domain::crdt::doc::{CrdtDoc, DocError, VersionVector};
use crate::domain::crdt::room;
use crate::domain::crdt::room_actor;
use crate::entities::{pages, sessions, toc_entries};
use crate::loro::toc::LoroTocDoc;
use crate::wire::broadcast::encode_broadcast;
use crate::wire::fragmenter::BatchFragmenter;

use super::toc_snapshot::{PageInfo, restore_toc, snapshot_toc};

// --- Actor ---

/// The TocActor coordinates the campaign's table of contents.
///
/// Under the hood, it uses the [`LoroTocDoc`] to store the
/// table of contents.
///
/// For agents, it backs the `ls` command.
///
/// TODO: Suggestion hydration on checkout. The restore path should:
/// 1. Rebuild the LoroTree from SQLite with structural entries (Folder/Page).
///    This gives clients a fast first paint.
/// 2. Query SQLite for pending suggestions on this campaign's ToC.
/// 3. Apply them as CRDT updates: inline suggestions (change/delete) go into
///    node metadata; new-entry suggestions become `kind: "suggestion"` nodes.
/// 4. These updates stream to clients via loro-protocol sync.
///
/// During the active session, the CRDT is authoritative. Suggestions arrive
/// via actor messages (from AgentConversation actors) and are applied directly
/// to the LoroDoc. The debounce timer writes everything back to SQLite.
/// On eviction, a final snapshot persists to SQLite and the doc is dropped.
pub struct TocActor {
    campaign_id: CampaignId,
    doc_room: room::Room<LoroTocDoc>,
    /// Maps Loro TreeIDs to stable toc_entry row ULIDs. Populated on
    /// restore, extended when new nodes are created during the session,
    /// consumed by `snapshot_toc` to produce upsert-friendly rows.
    id_map: HashMap<TreeID, String>,
    /// Page IDs currently known to the campaign. Used by `snapshot_toc`
    /// to filter out dangling references. Updated as Pages are
    /// created/deleted during the session.
    known_pages: HashSet<PageId>,
    db_writer: ActorRef<DatabaseWriteActor>,
    self_ref: ActorRef<TocActor>,
    /// Whether the doc has unpersisted edits and, if so, the armed flush timer.
    /// See [`Persist`]; the timer is inseparable from dirtiness by construction.
    /// `pub(super)` so the test-only `InspectDirty` probe in `tests.rs` can read
    /// dirtiness; folder-scoped, sealed from the rest of the crate.
    pub(super) persist: Persist,
    // Wait this long before persisting dirty changes to the database.
    debounce_duration: Duration,
    fragmenter: BatchFragmenter,
}

pub struct TocActorArgs {
    pub campaign_id: CampaignId,
    pub db_reader: DatabaseConnection,
    pub db_writer: ActorRef<DatabaseWriteActor>,
    pub debounce_duration: Duration,
}

impl Actor for TocActor {
    type Args = TocActorArgs;
    type Error = sea_orm::DbErr;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %args.campaign_id.0),
    )]
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let toc_rows = toc_entries::Entity::find()
            .order_by_asc(toc_entries::Column::Position)
            .all(&args.db_reader)
            .await
            .inspect_err(|e| tracing::error!(error = %e, "failed to query toc_entries"))?;

        let page_rows = pages::Entity::find()
            .all(&args.db_reader)
            .await
            .inspect_err(|e| tracing::error!(error = %e, "failed to query pages"))?;

        // Session ordinals, keyed by the page they document. Sessions are sparse
        // (most pages are entities), so this is a small map; a page absent here
        // simply has no ordinal.
        let session_ordinals: HashMap<PageId, i64> = sessions::Entity::find()
            .all(&args.db_reader)
            .await
            .inspect_err(|e| tracing::error!(error = %e, "failed to query sessions"))?
            .into_iter()
            .filter_map(|s| s.page_id.map(|pid| (PageId::from(pid), s.ordinal)))
            .collect();

        tracing::debug!(
            toc_entries = toc_rows.len(),
            pages = page_rows.len(),
            sessions = session_ordinals.len(),
            "restoring toc"
        );

        let pages_map: HashMap<PageId, PageInfo> = page_rows
            .into_iter()
            .filter_map(|t| {
                let page_id = PageId::from(t.id);
                // Build the page-kind sum; a session pulls its ordinal from the
                // temporal map. A session page with no temporal row is a data-
                // integrity violation (genesis writes both atomically), so log
                // and skip it -- it self-heals once the row exists.
                let page_kind = match PageKind::from(t.kind) {
                    PageKind::Entity => TocPageKind::Entity,
                    PageKind::Template => TocPageKind::Template,
                    PageKind::Session => match session_ordinals.get(&page_id) {
                        Some(&ordinal) => TocPageKind::Session { ordinal },
                        None => {
                            tracing::error!(
                                page_id = %page_id.0,
                                "session page has no temporal row; skipping in toc restore"
                            );
                            return None;
                        }
                    },
                };
                Some((
                    page_id,
                    PageInfo {
                        name: t.name,
                        status: t.status.into(),
                        page_kind,
                    },
                ))
            })
            .collect();

        let known_pages: HashSet<PageId> = pages_map.keys().cloned().collect();
        let (doc, id_map, dirty) = restore_toc(toc_rows, &pages_map);
        let doc_room = room::Room::new(doc);

        tracing::debug!(
            tree_size = id_map.len(),
            known_pages = known_pages.len(),
            dirty,
            "toc actor started"
        );

        let mut the_self = Self {
            campaign_id: args.campaign_id,
            doc_room,
            id_map,
            known_pages,
            db_writer: args.db_writer,
            self_ref: actor_ref,
            persist: Persist::new(),
            debounce_duration: args.debounce_duration,
            fragmenter: BatchFragmenter::new(250 * 1024),
        };
        if dirty {
            tracing::info!("toc restored with orphans, marked dirty");
            the_self
                .persist
                .schedule(&the_self.self_ref, the_self.debounce_duration);
        }
        Ok(the_self)
    }

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        _reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        if self.persist.is_dirty() {
            if let Err(err) = self.flush().await {
                tracing::error!(error=%err, "failed to persist toc on stop");
            }
        } else {
            tracing::debug!("toc clean, no snapshot needed on stop");
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CRDT Room
// ---------------------------------------------------------------------------

impl Message<room_actor::ClientJoin> for TocActor {
    type Reply = Result<room_actor::JoinResponse, room_actor::JoinError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, client_id = msg.client.0),
    )]
    async fn handle(
        &mut self,
        msg: room_actor::ClientJoin,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        use familiar_systems_app_shared::campaigns::internal::CampaignRole;
        // TODO: per-Page write access for players. When PageActor lands,
        // the mapping here should check Page-level permissions, not just
        // the campaign role.
        let capability = match msg.role {
            CampaignRole::Gm => room_actor::Capability::Write,
            CampaignRole::Player => room_actor::Capability::Read,
        };
        self.doc_room.on_join(msg.client, msg.tx, capability)
    }
}

impl Message<room_actor::ClientLeave> for TocActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, client_id = msg.client.0),
    )]
    async fn handle(
        &mut self,
        msg: room_actor::ClientLeave,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.doc_room.on_leave(msg.client);
    }
}

impl Message<room_actor::ClientUpdate> for TocActor {
    type Reply = Result<room_actor::AckPayload, room_actor::UpdateError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, client_id = msg.client.0),
    )]
    async fn handle(
        &mut self,
        msg: room_actor::ClientUpdate,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let old_version = self.doc_room.doc().version();
        let (broadcast, ack) = self.doc_room.apply_updates(msg.client, &msg.updates)?;
        if old_version != ack.version {
            self.persist
                .schedule(&self.self_ref, self.debounce_duration);
        }
        let frames = encode_broadcast(
            loro_protocol::CrdtType::Loro,
            "toc",
            &broadcast.updates,
            &self.fragmenter,
        );
        self.doc_room.fan_out(&frames, broadcast.exclude);
        Ok(ack)
    }
}

// ---------------------------------------------------------------------------
// Page-node mutations (server-initiated, from CampaignSupervisor::CreatePage)
// ---------------------------------------------------------------------------

/// Resolve a Page's ToC node, if present. The supervisor uses this to
/// validate a requested parent placement before any write happens.
#[derive(Debug, Clone)]
pub struct ResolvePageNode(pub PageId);

impl Message<ResolvePageNode> for TocActor {
    type Reply = Option<TreeID>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.0.0),
    )]
    async fn handle(
        &mut self,
        msg: ResolvePageNode,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.doc_room.doc().find_page_node(&msg.0)
    }
}

/// Insert a Page node into the live ToC and broadcast the change. Sent once
/// per Page creation, after the genesis row is committed. Appends at the ToC
/// root when `parent` is `None`, otherwise as the last child of the parent
/// Page's node. A `parent` that no longer resolves (a rare race after the
/// supervisor's pre-check) falls back to the root with a warning, rather than
/// failing a Page that is already persisted.
#[derive(Debug, Clone)]
pub struct AddPageNode {
    pub page_id: PageId,
    pub title: String,
    /// The page's kind (a session carries its ordinal here), so the new ToC node
    /// carries it for display composition.
    pub page_kind: TocPageKind,
    pub visibility: Status,
    pub parent: Option<PageId>,
}

impl Message<AddPageNode> for TocActor {
    // `ask`-invoked from the page-creation path, which treats a failure as
    // best-effort (logs, keeps the persisted Page). An `ask` handler's `Err` is
    // delivered back through the reply channel, so a typed error here is safe --
    // unlike a `tell` handler, where it would trip `on_panic` and stop the actor.
    type Reply = Result<(), DocError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.page_id.0),
    )]
    async fn handle(
        &mut self,
        msg: AddPageNode,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let parent_tree = match &msg.parent {
            None => None,
            Some(parent_id) => {
                let resolved = self.doc_room.doc().find_page_node(parent_id);
                if resolved.is_none() {
                    tracing::warn!(
                        parent = %parent_id.0,
                        "parent page node not found in toc; appending at root"
                    );
                }
                resolved
            }
        };

        let entry = TocEntry::Page {
            title: msg.title,
            page_id: msg.page_id.clone(),
            page_kind: msg.page_kind,
            visibility: msg.visibility,
            suggestions: Vec::new(),
        };

        let delta = self.doc_room.doc_mut().add_entry(parent_tree, &entry)?.0;

        // Track the new Page so `snapshot_toc` doesn't treat its node as a
        // dangling reference and drop it on the next persist.
        self.known_pages.insert(msg.page_id);

        let frames = encode_broadcast(
            loro_protocol::CrdtType::Loro,
            "toc",
            std::slice::from_ref(&delta),
            &self.fragmenter,
        );
        self.doc_room.fan_out(&frames, None);

        self.persist
            .schedule(&self.self_ref, self.debounce_duration);
        Ok(())
    }
}

/// A page to place under the folder created by [`SeedTocFolder`].
#[derive(Debug, Clone)]
pub struct SeedTocChild {
    pub page_id: PageId,
    pub title: String,
    pub page_kind: TocPageKind,
    pub visibility: Status,
}

/// Server-initiated: create a folder at the ToC root and place a batch of pages
/// under it in one shot. Used by template-bundle seeding at campaign creation.
///
/// The folder's `TreeID` never leaves the actor, so `AddPageNode`'s `parent`
/// stays a `PageId` (a folder has none) rather than growing a folder-addressing
/// variant for this one caller. Best-effort like [`AddPageNode`]: a failure
/// leaves the already-persisted pages to re-surface at the root on the next
/// checkout (the folder itself isn't persisted until the debounce flush, so a
/// mid-batch failure simply doesn't schedule one).
#[derive(Debug, Clone)]
pub struct SeedTocFolder {
    pub folder_title: String,
    pub folder_visibility: Status,
    pub children: Vec<SeedTocChild>,
}

impl Message<SeedTocFolder> for TocActor {
    // `ask`-invoked and best-effort, like `AddPageNode`: a typed `Err` returns
    // through the reply channel (not `on_panic`), so the seeder logs and the ToC
    // self-heals on checkout.
    type Reply = Result<(), DocError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, child_count = msg.children.len()),
    )]
    async fn handle(
        &mut self,
        msg: SeedTocFolder,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let folder = TocEntry::Folder {
            title: msg.folder_title,
            visibility: msg.folder_visibility,
            suggestions: Vec::new(),
        };
        let (folder_delta, folder_tree) = self.doc_room.doc_mut().add_entry(None, &folder)?;

        let mut deltas = Vec::with_capacity(msg.children.len() + 1);
        deltas.push(folder_delta);
        for child in msg.children {
            let entry = TocEntry::Page {
                title: child.title,
                page_id: child.page_id.clone(),
                page_kind: child.page_kind,
                visibility: child.visibility,
                suggestions: Vec::new(),
            };
            let (delta, _) = self
                .doc_room
                .doc_mut()
                .add_entry(Some(folder_tree), &entry)?;
            deltas.push(delta);
            // Track each new Page so `snapshot_toc` keeps its node instead of
            // dropping it as a dangling reference on the next flush.
            self.known_pages.insert(child.page_id);
        }

        let frames = encode_broadcast(
            loro_protocol::CrdtType::Loro,
            "toc",
            &deltas,
            &self.fragmenter,
        );
        self.doc_room.fan_out(&frames, None);

        self.persist
            .schedule(&self.self_ref, self.debounce_duration);
        Ok(())
    }
}

/// Whenever a [`PageActor`](crate::actors::page::PageActor) changes any of these
/// fields, the ToC receives the update on a best-effort basis and broadcasts it
/// to all clients.
///
/// The PageActor spawns the push, so it never blocks the edit path. `created_at`
/// (the pushing actor incarnation's spawn time) and `version` (the Page doc's
/// version at send time) together order updates so the ToC can (eventually) drop
/// stale, out-of-order pushes: a later `created_at` dominates, and `version`
/// breaks ties within one incarnation. See the version-gating TODO in the handler.
///
/// If we fail to accept this message it's not fatal: the
/// [`LoroPageDoc`](crate::loro::page::LoroPageDoc) is the authoritative truth, so
/// it self-heals on the next checkout, or the next time a relevant field changes.
///
/// TODO: Add the `icon` field once Page icons exist.
#[derive(Debug, Clone)]
pub struct UpdatePageNode {
    pub page_id: PageId,
    pub title: String,
    pub visibility: Status,
    /// The pushing actor incarnation's spawn time. Dominates `version` when
    /// ordering updates: a respawn resets the vv lineage, so vv alone cannot rank
    /// updates across incarnations.
    pub created_at: DateTime<Utc>,
    pub version: VersionVector,
}

impl Message<UpdatePageNode> for TocActor {
    // `ask`-invoked from a task the PageActor spawns: the edit path never blocks
    // on the ToC, and a returned `Err` is delivered to that task's logging (an
    // `ask` reply, not `on_panic`). So the error is honest and typed, not
    // swallowed and not actor-fatal.
    type Reply = Result<(), DocError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.page_id.0),
    )]
    async fn handle(
        &mut self,
        msg: UpdatePageNode,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let Some(tree_id) = self.doc_room.doc().find_page_node(&msg.page_id) else {
            tracing::trace!("update for a page not in the toc; ignoring (self-heals on checkout)");
            return Ok(());
        };

        // TODO(version-gating): order updates by `(msg.created_at, msg.version)`
        // and drop stale ones. Pushes are spawned (can reorder) and an
        // evict+respawn resets the Page doc's vv lineage (pushes cross
        // incarnations), so neither arrival order nor vv alone is enough. Desired:
        // track the last-applied `(created_at, version)` per node as an `Option`:
        //   * `None`             => first sighting; apply.
        //   * newer `created_at` => newer incarnation; apply (vv ignored).
        //   * older `created_at` => stale incarnation; drop.
        //   * equal `created_at` => same incarnation; apply iff `version` is
        //                           causally newer than the stored vv.
        // For now we apply unconditionally and accept the race.
        tracing::trace!(
            version_bytes = msg.version.0.len(),
            created_at = %msg.created_at,
            "applying toc node update (version-gating not yet wired)"
        );

        // Read the current entry once: it tells us whether visibility changed
        // (the only field that needs a DB write) and carries the immutable
        // kind/ordinal we must preserve. This push only mutates title/visibility,
        // so threading kind/ordinal through the PageActor would be redundant --
        // the ToC already holds them.
        let existing = self.doc_room.doc().read_entry(tree_id);
        // `find_page_node` already proved this resolves to a Page node, so a
        // non-Page (or unreadable) entry here means the ToC doc is corrupt. Do not
        // invent a kind: defaulting to `Entity` would silently strip a session's
        // ordinal from the live tree until the next checkout healed it. Refuse to
        // guess -- log loudly and skip the write (it self-heals on checkout, like
        // the not-in-toc case above). We moved to Rust to make "shouldn't happen"
        // a handled case, not undefined behavior.
        let Some(TocEntry::Page {
            page_kind,
            visibility,
            ..
        }) = &existing
        else {
            tracing::error!(
                entry = ?existing,
                "UpdatePageNode: node resolved as a page but read back as a non-page \
                 entry; refusing to default its kind. Skipping; self-heals on checkout."
            );
            return Ok(());
        };
        let visibility_changed = *visibility != msg.visibility;
        let page_kind = page_kind.clone();

        let entry = TocEntry::Page {
            title: msg.title,
            page_id: msg.page_id,
            page_kind,
            visibility: msg.visibility,
            suggestions: Vec::new(),
        };
        let delta = self.doc_room.doc_mut().update_entry(tree_id, &entry)?;

        let frames = encode_broadcast(
            loro_protocol::CrdtType::Loro,
            "toc",
            std::slice::from_ref(&delta),
            &self.fragmenter,
        );
        self.doc_room.fan_out(&frames, None);

        if visibility_changed {
            self.persist
                .schedule(&self.self_ref, self.debounce_duration);
        }
        Ok(())
    }
}

/// Test-only probe: read the current title of a Page's live ToC node. Lives at
/// module scope (not in `mod tests`) so the `page` actor's integration test can
/// assert the server-authoritative title push end-to-end.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct ReadPageNodeTitle(pub PageId);

#[cfg(test)]
impl Message<ReadPageNodeTitle> for TocActor {
    type Reply = Option<String>;

    async fn handle(
        &mut self,
        msg: ReadPageNodeTitle,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let tree_id = self.doc_room.doc().find_page_node(&msg.0)?;
        match self.doc_room.doc().read_entry(tree_id)? {
            TocEntry::Page { title, .. } => Some(title),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

impl TocActor {
    /// Serialize the ToC tree to rows and write them durably, awaiting the
    /// commit (`ask`, not `tell`). The error is returned so the caller keeps the
    /// actor dirty and retries; this never silently clears dirtiness.
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn flush(&mut self) -> Result<(), PersistError> {
        let rows = snapshot_toc(self.doc_room.doc(), &mut self.id_map, &self.known_pages);
        let row_count = rows.len();
        tracing::debug!(row_count, "persisting toc snapshot");

        self.db_writer.ask(WriteTocSnapshot { rows }).await?;

        tracing::debug!(row_count, "toc snapshot written");
        Ok(())
    }
}

impl Message<PersistNow> for TocActor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
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
