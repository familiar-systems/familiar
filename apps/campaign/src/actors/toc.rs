use chrono::{DateTime, Utc};
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::loro::toc::TocEntry;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use loro::TreeID;
use sea_orm::ActiveValue::Set;
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder};
use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::actors::database_writer::{DatabaseWriteActor, WriteTocSnapshot};
use crate::actors::persist::{Persist, PersistError, PersistNow};
use crate::domain::crdt::doc::{CrdtDoc, DocError, VersionVector};
use crate::domain::crdt::room;
use crate::domain::crdt::room_actor;
use crate::entities::columns::{PageIdCol, StatusCol};
use crate::entities::{pages, toc_entries};
use crate::loro::toc::{LoroTocDoc, TocTreeNode};
use crate::wire::broadcast::encode_broadcast;
use crate::wire::fragmenter::BatchFragmenter;

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
    persist: Persist,
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

        tracing::debug!(
            toc_entries = toc_rows.len(),
            pages = page_rows.len(),
            "restoring toc"
        );

        let pages_map: HashMap<PageId, PageInfo> = page_rows
            .into_iter()
            .map(|t| {
                (
                    PageId::from(t.id),
                    PageInfo {
                        name: t.name,
                        status: t.status.into(),
                    },
                )
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

        // Only a visibility change needs a DB write: titles live in `pages.name`
        // and are re-derived on checkout, never persisted from the ToC side.
        let visibility_changed = self
            .doc_room
            .doc()
            .read_entry(tree_id)
            .map(|entry| *entry.visibility() != msg.visibility)
            .unwrap_or(true);

        let entry = TocEntry::Page {
            title: msg.title,
            page_id: msg.page_id,
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

// ---------------------------------------------------------------------------
// Persistence: snapshot & restore
// ---------------------------------------------------------------------------

/// Info about a Page needed during ToC restore (title and visibility).
pub struct PageInfo {
    pub name: String,
    pub status: Status,
}

/// Serialize the current LoroTree state into flat rows for persistence.
///
/// Uses `id_map` to assign stable row IDs: existing nodes keep their row
/// ID from the last restore/snapshot, new nodes get a fresh ULID (and are
/// inserted into `id_map` for future cycles). This makes the output
/// suitable for `INSERT OR REPLACE` rather than full-table replacement.
///
/// Skips `TocEntry::Suggestion` entries (deferred) and `TocEntry::Page`
/// entries whose `page_id` is not in `known_pages` (dangling references).
pub fn snapshot_toc(
    doc: &LoroTocDoc,
    id_map: &mut HashMap<TreeID, String>,
    known_pages: &HashSet<PageId>,
) -> Vec<toc_entries::ActiveModel> {
    let tree = doc.read_tree();
    let mut rows = Vec::new();
    collect_rows(&tree, None, id_map, known_pages, &mut rows);
    rows
}

fn collect_rows(
    nodes: &[TocTreeNode],
    parent_row_id: Option<String>,
    id_map: &mut HashMap<TreeID, String>,
    known_pages: &HashSet<PageId>,
    out: &mut Vec<toc_entries::ActiveModel>,
) {
    let mut position: i32 = 0;
    for node in nodes {
        let (page_id, folder_title) = match &node.entry {
            TocEntry::Folder { title, .. } => (None, Some(title.clone())),
            TocEntry::Page { page_id, .. } => {
                if !known_pages.contains(page_id) {
                    continue;
                }
                (Some(PageIdCol::from(page_id.clone())), None)
            }
            TocEntry::Suggestion { .. } => continue,
        };

        let row_id = id_map
            .entry(node.tree_id)
            .or_insert_with(|| ulid::Ulid::new().to_string())
            .clone();

        let visibility = StatusCol::from(*node.entry.visibility());

        out.push(toc_entries::ActiveModel {
            id: Set(row_id.clone()),
            page_id: Set(page_id),
            folder_title: Set(folder_title),
            visibility: Set(visibility),
            parent_id: Set(parent_row_id.clone()),
            position: Set(position),
        });
        position += 1;

        collect_rows(&node.children, Some(row_id), id_map, known_pages, out);
    }
}

/// Rebuild a `LoroTocDoc` from persisted rows and the current Pages in the DB.
///
/// Entries are inserted in topological order (parents before children, siblings
/// by position). Page titles come from the `pages` map, not from the
/// toc_entries table, so they're always fresh.
///
/// Returns `(doc, id_map, dirty)`:
/// - `id_map`: the `TreeID -> row ID` mapping for future snapshots.
/// - `dirty`: true when orphan Pages were appended (caller should writeback).
///
/// Any Page present in `pages` but not referenced by any toc_entry is an
/// orphan: appended to root with its own visibility.
pub fn restore_toc(
    entries: Vec<toc_entries::Model>,
    pages: &HashMap<PageId, PageInfo>,
) -> (LoroTocDoc, HashMap<TreeID, String>, bool) {
    let mut doc = LoroTocDoc::new();
    let sorted = topological_sort(entries);

    // row_id -> TreeID for parent lookups during restore
    let mut row_to_tree: HashMap<String, TreeID> = HashMap::new();
    // TreeID -> row_id returned for future snapshots
    let mut tree_to_row: HashMap<TreeID, String> = HashMap::new();

    let mut referenced_pages: HashSet<PageId> = HashSet::new();

    for entry in &sorted {
        let parent_tree_id = entry
            .parent_id
            .as_ref()
            .and_then(|pid| row_to_tree.get(pid))
            .copied();

        let toc_entry = match row_to_entry(entry, pages) {
            Some(e) => e,
            None => continue,
        };

        if let Some(ref tid) = entry.page_id {
            referenced_pages.insert(PageId::from(tid.clone()));
        }

        let (_, tree_id) = doc
            .add_entry(parent_tree_id, &toc_entry)
            .expect("add entry during restore");
        row_to_tree.insert(entry.id.clone(), tree_id);
        tree_to_row.insert(tree_id, entry.id.clone());
    }

    let mut dirty = false;
    for (page_id, info) in pages {
        if !referenced_pages.contains(page_id) {
            let entry = TocEntry::Page {
                title: info.name.clone(),
                page_id: page_id.clone(),
                visibility: info.status,
                suggestions: Vec::new(),
            };
            let (_, tree_id) = doc
                .add_entry(None, &entry)
                .expect("add orphan during restore");
            tree_to_row.insert(tree_id, ulid::Ulid::new().to_string());
            dirty = true;
        }
    }

    (doc, tree_to_row, dirty)
}

/// Convert a toc_entry row into a `TocEntry`.
/// Returns `None` for Page entries whose page_id is missing from the map
/// (deleted between writeback and restore).
fn row_to_entry(row: &toc_entries::Model, pages: &HashMap<PageId, PageInfo>) -> Option<TocEntry> {
    let visibility: Status = row.visibility.into();

    if let Some(ref page_id_col) = row.page_id {
        let page_id = PageId::from(page_id_col.clone());
        let info = pages.get(&page_id)?;
        Some(TocEntry::Page {
            title: info.name.clone(),
            page_id,
            visibility,
            suggestions: Vec::new(),
        })
    } else {
        Some(TocEntry::Folder {
            title: row.folder_title.clone().unwrap_or_default(),
            visibility,
            suggestions: Vec::new(),
        })
    }
}

/// Sort entries so that parents appear before their children.
///
/// Within each parent group, entries retain their input order (which should
/// already be sorted by `position` from the SQL query). For a well-ordered
/// input this completes in a single pass.
fn topological_sort(entries: Vec<toc_entries::Model>) -> Vec<toc_entries::Model> {
    let mut sorted = Vec::with_capacity(entries.len());
    let mut remaining = entries;
    let mut processed_ids: HashSet<String> = HashSet::new();

    while !remaining.is_empty() {
        let before_len = remaining.len();
        let mut next_remaining = Vec::new();

        for entry in remaining {
            let can_process = match &entry.parent_id {
                None => true,
                Some(pid) => processed_ids.contains(pid),
            };

            if can_process {
                processed_ids.insert(entry.id.clone());
                sorted.push(entry);
            } else {
                next_remaining.push(entry);
            }
        }

        remaining = next_remaining;

        if remaining.len() == before_len {
            // No progress: remaining entries have invalid parent_ids. Drop them.
            break;
        }
    }

    sorted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::columns::PageKindCol;
    use familiar_systems_campaign_shared::loro::toc::TocEntryKind;

    /// Test-only probe: ask the actor whether it currently holds unpersisted
    /// edits, so a test can assert dirtiness without reaching into private state.
    #[derive(Debug, Clone, Copy)]
    struct InspectDirty;

    impl Message<InspectDirty> for TocActor {
        type Reply = bool;
        async fn handle(
            &mut self,
            _: InspectDirty,
            _ctx: &mut Context<Self, Self::Reply>,
        ) -> Self::Reply {
            self.persist.is_dirty()
        }
    }

    fn make_folder(title: &str) -> TocEntry {
        TocEntry::Folder {
            title: title.to_string(),
            visibility: Status::Known,
            suggestions: Vec::new(),
        }
    }

    fn make_page(title: &str) -> (TocEntry, PageId) {
        let page_id = PageId::generate();
        let entry = TocEntry::Page {
            title: title.to_string(),
            page_id: page_id.clone(),
            visibility: Status::Known,
            suggestions: Vec::new(),
        };
        (entry, page_id)
    }

    fn active_to_model(am: toc_entries::ActiveModel) -> toc_entries::Model {
        toc_entries::Model {
            id: am.id.unwrap(),
            page_id: am.page_id.unwrap(),
            folder_title: am.folder_title.unwrap(),
            visibility: am.visibility.unwrap(),
            parent_id: am.parent_id.unwrap(),
            position: am.position.unwrap(),
        }
    }

    fn page_info(name: &str, status: Status) -> PageInfo {
        PageInfo {
            name: name.to_string(),
            status,
        }
    }

    fn pages_map(items: &[(PageId, &str, Status)]) -> HashMap<PageId, PageInfo> {
        items
            .iter()
            .map(|(id, name, status)| (id.clone(), page_info(name, *status)))
            .collect()
    }

    /// Collect the row IDs from a set of ActiveModels.
    fn row_ids(rows: &[toc_entries::ActiveModel]) -> HashSet<String> {
        rows.iter().map(|r| r.id.clone().unwrap()).collect()
    }

    // -- Round-trip tests --

    #[test]
    fn round_trip_empty_tree() {
        let doc = LoroTocDoc::new();
        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &HashSet::new());
        assert!(rows.is_empty());

        let (restored, _, dirty) = restore_toc(Vec::new(), &HashMap::new());
        assert!(restored.read_tree().is_empty());
        assert!(!dirty);
    }

    #[test]
    fn round_trip_flat_entries() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(None, &make_folder("Act I")).unwrap();

        let (page_entry, page_id) = make_page("The Iron Citadel");
        doc.add_entry(None, &page_entry).unwrap();

        let pages = pages_map(&[(page_id.clone(), "The Iron Citadel", Status::Known)]);
        let known: HashSet<PageId> = pages.keys().cloned().collect();

        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &known);
        assert_eq!(rows.len(), 2);

        let models: Vec<_> = rows.into_iter().map(active_to_model).collect();
        let (restored, _, dirty) = restore_toc(models, &pages);
        assert!(!dirty);

        let original = doc.read_tree();
        let round_tripped = restored.read_tree();
        assert_eq!(original.len(), round_tripped.len());
        assert_eq!(original[0].entry, round_tripped[0].entry);
        assert_eq!(original[1].entry, round_tripped[1].entry);
    }

    #[test]
    fn round_trip_nested_tree() {
        let mut doc = LoroTocDoc::new();
        let (_, folder_id) = doc.add_entry(None, &make_folder("Act I")).unwrap();

        let (child1, tid1) = make_page("The Dragon's Lair");
        doc.add_entry(Some(folder_id), &child1).unwrap();

        let (child2, tid2) = make_page("The Crystal Cave");
        let (_, child2_id) = doc.add_entry(Some(folder_id), &child2).unwrap();

        let (grandchild, tid3) = make_page("The Hidden Alcove");
        doc.add_entry(Some(child2_id), &grandchild).unwrap();

        let pages = pages_map(&[
            (tid1, "The Dragon's Lair", Status::Known),
            (tid2, "The Crystal Cave", Status::Known),
            (tid3, "The Hidden Alcove", Status::Known),
        ]);
        let known: HashSet<PageId> = pages.keys().cloned().collect();

        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &known);
        assert_eq!(rows.len(), 4);

        let models: Vec<_> = rows.into_iter().map(active_to_model).collect();
        let (restored, _, dirty) = restore_toc(models, &pages);
        assert!(!dirty);

        let orig = doc.read_tree();
        let rest = restored.read_tree();
        assert_eq!(orig.len(), 1, "one root folder");
        assert_eq!(rest.len(), 1);
        assert_eq!(orig[0].children.len(), 2, "two children");
        assert_eq!(rest[0].children.len(), 2);
        assert_eq!(
            orig[0].children[1].children.len(),
            1,
            "one grandchild under second child"
        );
        assert_eq!(rest[0].children[1].children.len(), 1);
        assert_eq!(
            orig[0].children[1].children[0].entry,
            rest[0].children[1].children[0].entry
        );
    }

    // -- Dangling page_id test --

    #[test]
    fn snapshot_drops_dangling_page_entries() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(None, &make_folder("Keeper")).unwrap();

        let (dangling, _) = make_page("Ghost Entry");
        doc.add_entry(None, &dangling).unwrap();

        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &HashSet::new());
        assert_eq!(rows.len(), 1, "only the folder survives");
        assert_eq!(
            active_to_model(rows.into_iter().next().unwrap())
                .folder_title
                .as_deref(),
            Some("Keeper")
        );
    }

    #[test]
    fn snapshot_preserves_sibling_positions_after_filtering() {
        let mut doc = LoroTocDoc::new();
        let (page_a, tid_a) = make_page("Alpha");
        let (page_b, _) = make_page("Dangling Beta");
        let (page_c, tid_c) = make_page("Gamma");
        doc.add_entry(None, &page_a).unwrap();
        doc.add_entry(None, &page_b).unwrap();
        doc.add_entry(None, &page_c).unwrap();

        let mut known = HashSet::new();
        known.insert(tid_a.clone());
        known.insert(tid_c.clone());

        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &known);
        assert_eq!(rows.len(), 2);
        let models: Vec<_> = rows.into_iter().map(active_to_model).collect();
        assert_eq!(models[0].position, 0, "Alpha at position 0");
        assert_eq!(models[1].position, 1, "Gamma renumbered to position 1");
    }

    // -- Orphan detection tests --

    #[test]
    fn orphan_pages_appended_to_root() {
        let orphan_id = PageId::generate();
        let pages = pages_map(&[(orphan_id.clone(), "Lost Artifact", Status::GmOnly)]);

        let (doc, id_map, dirty) = restore_toc(Vec::new(), &pages);
        assert!(dirty);
        assert_eq!(id_map.len(), 1, "orphan gets an entry in the id_map");

        let tree = doc.read_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry.kind(), TocEntryKind::Page);
        assert_eq!(tree[0].entry.title(), Some("Lost Artifact"));
        assert_eq!(*tree[0].entry.visibility(), Status::GmOnly);
    }

    #[test]
    fn orphan_inherits_page_visibility() {
        let id = PageId::generate();
        let pages = pages_map(&[(id.clone(), "Public NPC", Status::Known)]);

        let (doc, _, _) = restore_toc(Vec::new(), &pages);
        assert_eq!(*doc.read_tree()[0].entry.visibility(), Status::Known);
    }

    // -- Restore edge cases --

    #[test]
    fn restore_skips_entries_with_deleted_pages() {
        let stale_page_id = PageIdCol::from(PageId::generate());
        let row = toc_entries::Model {
            id: ulid::Ulid::new().to_string(),
            page_id: Some(stale_page_id),
            folder_title: None,
            visibility: StatusCol::Known,
            parent_id: None,
            position: 0,
        };

        let (doc, id_map, dirty) = restore_toc(vec![row], &HashMap::new());
        assert!(doc.read_tree().is_empty(), "stale entry is skipped");
        assert!(id_map.is_empty(), "skipped entry has no id_map entry");
        assert!(!dirty, "no orphans because the page is gone entirely");
    }

    #[test]
    fn topological_sort_handles_out_of_order_input() {
        let parent_id = ulid::Ulid::new().to_string();
        let child_id = ulid::Ulid::new().to_string();

        let child = toc_entries::Model {
            id: child_id.clone(),
            page_id: None,
            folder_title: Some("Child".to_string()),
            visibility: StatusCol::Known,
            parent_id: Some(parent_id.clone()),
            position: 0,
        };
        let parent = toc_entries::Model {
            id: parent_id.clone(),
            page_id: None,
            folder_title: Some("Parent".to_string()),
            visibility: StatusCol::Known,
            parent_id: None,
            position: 0,
        };

        let sorted = topological_sort(vec![child, parent]);
        assert_eq!(sorted.len(), 2);
        assert_eq!(sorted[0].id, parent_id, "parent comes first");
        assert_eq!(sorted[1].id, child_id, "child comes second");
    }

    #[test]
    fn topological_sort_drops_entries_with_invalid_parents() {
        let orphan_row = toc_entries::Model {
            id: ulid::Ulid::new().to_string(),
            page_id: None,
            folder_title: Some("Lost".to_string()),
            visibility: StatusCol::Known,
            parent_id: Some("nonexistent".to_string()),
            position: 0,
        };

        let sorted = topological_sort(vec![orphan_row]);
        assert!(sorted.is_empty(), "entry with invalid parent_id is dropped");
    }

    // -- Snapshot skips suggestions --

    #[test]
    fn snapshot_skips_suggestion_entries() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(None, &make_folder("Real Content")).unwrap();

        let suggestion = TocEntry::Suggestion {
            conversation_id: familiar_systems_campaign_shared::id::ConversationId(ulid::Ulid::new()),
            title: Some("Proposed NPC".to_string()),
            visibility: Status::GmOnly,
        };
        doc.add_entry(None, &suggestion).unwrap();

        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &HashSet::new());
        assert_eq!(rows.len(), 1, "only the folder, not the suggestion");
    }

    // -- Stable ID tests --

    #[test]
    fn snapshot_reuses_ids_from_restore() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(None, &make_folder("Stable")).unwrap();
        let (page_entry, tid) = make_page("Also Stable");
        doc.add_entry(None, &page_entry).unwrap();

        let pages = pages_map(&[(tid.clone(), "Also Stable", Status::Known)]);
        let known: HashSet<PageId> = pages.keys().cloned().collect();

        // First snapshot: generates fresh IDs.
        let mut id_map = HashMap::new();
        let rows1 = snapshot_toc(&doc, &mut id_map, &known);
        let ids1 = row_ids(&rows1);
        assert_eq!(ids1.len(), 2);

        // Restore from those rows, get a new id_map.
        let models: Vec<_> = rows1.into_iter().map(active_to_model).collect();
        let (restored, mut restored_id_map, _) = restore_toc(models, &pages);

        // Second snapshot on the restored doc: should reuse the same row IDs.
        let rows2 = snapshot_toc(&restored, &mut restored_id_map, &known);
        let ids2 = row_ids(&rows2);
        assert_eq!(ids1, ids2, "row IDs are stable across snapshot/restore");
    }

    #[test]
    fn new_node_gets_fresh_id() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(None, &make_folder("Original")).unwrap();

        let mut id_map = HashMap::new();
        let rows1 = snapshot_toc(&doc, &mut id_map, &HashSet::new());
        let original_id: String = rows1[0].id.clone().unwrap();

        // Add a new node after the first snapshot.
        doc.add_entry(None, &make_folder("Newcomer")).unwrap();

        let rows2 = snapshot_toc(&doc, &mut id_map, &HashSet::new());
        assert_eq!(rows2.len(), 2);

        let ids: Vec<String> = rows2.iter().map(|r| r.id.clone().unwrap()).collect();
        assert_eq!(ids[0], original_id, "existing node keeps its ID");
        assert_ne!(ids[1], original_id, "new node gets a different ID");
    }

    #[test]
    fn removed_node_absent_from_snapshot() {
        let mut doc = LoroTocDoc::new();
        let (_, keep_id) = doc.add_entry(None, &make_folder("Keep")).unwrap();
        let (_, remove_id) = doc.add_entry(None, &make_folder("Remove")).unwrap();

        let mut id_map = HashMap::new();
        let rows1 = snapshot_toc(&doc, &mut id_map, &HashSet::new());
        assert_eq!(rows1.len(), 2);
        let removed_row_id = id_map[&remove_id].clone();

        doc.remove_entry(remove_id).unwrap();

        let rows2 = snapshot_toc(&doc, &mut id_map, &HashSet::new());
        assert_eq!(rows2.len(), 1);
        let surviving_ids = row_ids(&rows2);
        assert!(
            !surviving_ids.contains(&removed_row_id),
            "removed node's row ID is absent"
        );
        assert!(
            surviving_ids.contains(&id_map[&keep_id]),
            "kept node's row ID is present"
        );
    }

    // -- Actor: AddPageNode --

    /// Proves the critical `known_pages` wiring: a node added via
    /// `AddPageNode` survives `snapshot_toc` (which drops Page entries not in
    /// `known_pages`) and is persisted. Forced through `on_stop` rather than
    /// the debounce timer for determinism.
    #[tokio::test]
    async fn add_page_node_tracks_known_page_and_persists() {
        use crate::actors::database_writer::{DatabaseWriteActor, DatabaseWriteActorArgs, Ping};
        use crate::db;
        use crate::migrations::Migrator;
        use chrono::Utc;
        use kameo::actor::Spawn;
        use sea_orm::ActiveModelTrait;
        use sea_orm_migration::MigratorTrait;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");

        let campaign_id = CampaignId::generate();
        let db_writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

        let toc = TocActor::spawn(TocActorArgs {
            campaign_id,
            db_reader: conn.clone(),
            db_writer: db_writer.clone(),
            debounce_duration: Duration::from_secs(60), // don't fire mid-test
        });
        toc.wait_for_startup().await;

        // Insert the backing Page row so the toc_entries FK is satisfied. The
        // running TocActor doesn't know about it yet (startup already read the
        // pages table), which is exactly the create-time situation.
        let page_id = PageId::generate();
        let now = Utc::now();
        pages::ActiveModel {
            id: Set(PageIdCol::from(page_id.clone())),
            name: Set("Korgath".into()),
            status: Set(StatusCol::from(Status::GmOnly)),
            kind: Set(PageKindCol::Entity),
            template_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .unwrap();

        toc.ask(AddPageNode {
            page_id: page_id.clone(),
            title: "Korgath".into(),
            visibility: Status::GmOnly,
            parent: None,
        })
        .await
        .expect("add page node");

        // Stopping flushes the dirty doc through on_stop -> WriteTocSnapshot.
        toc.stop_gracefully().await.unwrap();
        toc.wait_for_shutdown_with_result(|_| ()).await;
        // FIFO mailbox: Ping returns only after the snapshot write is processed.
        db_writer.ask(Ping).await.unwrap();

        let rows = toc_entries::Entity::find().all(&conn).await.unwrap();
        assert_eq!(rows.len(), 1, "the new page's toc entry was persisted");
        assert_eq!(
            rows[0].page_id.clone().map(PageId::from),
            Some(page_id),
            "persisted entry points at the created page"
        );
    }

    /// Bug 3 regression: a flush that fails (here, the writer is dead so every
    /// `ask` errors) must leave the actor dirty, not falsely clean. The old
    /// fire-and-forget path cleared `dirty` on enqueue regardless of the write's
    /// fate; the `ask`-and-`after_flush` path keeps it dirty so the on-stop flush
    /// and subsequent retries still fire.
    #[tokio::test]
    async fn failed_flush_leaves_actor_dirty() {
        use crate::actors::database_writer::{DatabaseWriteActor, DatabaseWriteActorArgs};
        use crate::db;
        use crate::migrations::Migrator;
        use kameo::actor::Spawn;
        use sea_orm_migration::MigratorTrait;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");

        let campaign_id = CampaignId::generate();
        let db_writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });

        // Kill the writer up front so every flush `ask` fails (ActorNotRunning).
        db_writer.stop_gracefully().await.unwrap();
        db_writer.wait_for_shutdown_with_result(|_| ()).await;

        let toc = TocActor::spawn(TocActorArgs {
            campaign_id,
            db_reader: conn.clone(),
            db_writer,
            debounce_duration: Duration::from_millis(30),
        });
        toc.wait_for_startup().await;

        // A server-side ToC mutation marks dirty and schedules a doomed flush.
        // AddPageNode touches only the in-memory doc, so no backing row needed.
        toc.ask(AddPageNode {
            page_id: PageId::generate(),
            title: "Korgath".into(),
            visibility: Status::GmOnly,
            parent: None,
        })
        .await
        .expect("add page node");

        // Let the debounce fire and the flush fail against the dead writer.
        tokio::time::sleep(Duration::from_millis(150)).await;

        assert!(
            toc.ask(InspectDirty).await.unwrap(),
            "a failed flush must leave the actor dirty"
        );

        toc.stop_gracefully().await.unwrap();
        toc.wait_for_shutdown_with_result(|_| ()).await;
    }

    // -- Actor: UpdatePageNode --

    /// The server-authoritative title path: a Page's owning actor pushes
    /// node-state changes here. A title-only change refreshes the live tree and
    /// broadcasts but schedules no snapshot (titles re-derive from `pages.name`);
    /// a visibility change additionally marks the actor dirty.
    #[tokio::test]
    async fn update_page_node_refreshes_title_and_persists_only_on_visibility() {
        use crate::actors::database_writer::DatabaseWriteActorArgs;
        use crate::db;
        use crate::migrations::Migrator;
        use chrono::Utc;
        use kameo::actor::Spawn;
        use sea_orm::ActiveModelTrait;
        use sea_orm_migration::MigratorTrait;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");

        // A Page plus a toc_entry referencing it: restore yields a clean tree
        // (no orphan), so persist-scheduling can be asserted precisely.
        let page_id = PageId::generate();
        let now = Utc::now();
        pages::ActiveModel {
            id: Set(PageIdCol::from(page_id.clone())),
            name: Set("Original".into()),
            status: Set(StatusCol::from(Status::GmOnly)),
            kind: Set(PageKindCol::Entity),
            template_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .unwrap();
        toc_entries::ActiveModel {
            id: Set(ulid::Ulid::new().to_string()),
            page_id: Set(Some(PageIdCol::from(page_id.clone()))),
            folder_title: Set(None),
            visibility: Set(StatusCol::from(Status::GmOnly)),
            parent_id: Set(None),
            position: Set(0),
        }
        .insert(&conn)
        .await
        .unwrap();

        let campaign_id = CampaignId::generate();
        let db_writer = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: campaign_id.clone(),
            conn: conn.clone(),
        });
        let toc = TocActor::spawn(TocActorArgs {
            campaign_id,
            db_reader: conn.clone(),
            db_writer,
            debounce_duration: Duration::from_secs(60), // don't fire mid-test
        });
        toc.wait_for_startup().await;

        assert_eq!(
            toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap(),
            Some("Original".to_string()),
            "restored node carries the page's name"
        );
        assert!(!toc.ask(InspectDirty).await.unwrap(), "clean after restore");

        // Title-only change: live tree updates, nothing scheduled to persist.
        toc.ask(UpdatePageNode {
            page_id: page_id.clone(),
            title: "Renamed".into(),
            visibility: Status::GmOnly,
            // Version-gating is not yet wired (see handler TODO); values are inert.
            created_at: Utc::now(),
            version: VersionVector(Vec::new()),
        })
        .await
        .expect("update page node");
        assert_eq!(
            toc.ask(ReadPageNodeTitle(page_id.clone())).await.unwrap(),
            Some("Renamed".to_string()),
            "live title updated"
        );
        assert!(
            !toc.ask(InspectDirty).await.unwrap(),
            "a title-only change must not schedule a snapshot"
        );

        // Visibility change: a snapshot is now warranted.
        toc.ask(UpdatePageNode {
            page_id: page_id.clone(),
            title: "Renamed".into(),
            visibility: Status::Known,
            created_at: Utc::now(),
            version: VersionVector(Vec::new()),
        })
        .await
        .expect("update page node visibility");
        assert!(
            toc.ask(InspectDirty).await.unwrap(),
            "a visibility change must schedule a snapshot"
        );

        toc.stop_gracefully().await.unwrap();
        toc.wait_for_shutdown_with_result(|_| ()).await;
    }
}
