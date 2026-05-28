use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::ThingId;
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
use crate::domain::crdt::doc::CrdtDoc;
use crate::domain::crdt::room;
use crate::domain::crdt::room_actor;
use crate::entities::columns::{StatusCol, ThingIdCol};
use crate::entities::{things, toc_entries};
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
/// 1. Rebuild the LoroTree from SQLite with structural entries (Folder/Thing).
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
    /// Thing IDs currently known to the campaign. Used by `snapshot_toc`
    /// to filter out dangling references. Updated as Things are
    /// created/deleted during the session.
    known_things: HashSet<ThingId>,
    db_writer: ActorRef<DatabaseWriteActor>,
    self_ref: ActorRef<TocActor>,
    /// If dirty, this has yet to be synced back to the database.
    dirty: bool,
    // Wait this long before persisting dirty changes to the database.
    debounce_duration: Duration,
    // Handle to the current persist task, if any.
    persist_timer: Option<tokio::task::JoinHandle<()>>,
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

    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let span = tracing::info_span!("toc_actor", campaign_id = %args.campaign_id.0);
        let _guard = span.enter();

        let toc_rows = toc_entries::Entity::find()
            .order_by_asc(toc_entries::Column::Position)
            .all(&args.db_reader)
            .await
            .inspect_err(|e| tracing::error!(error = %e, "failed to query toc_entries"))?;

        let thing_rows = things::Entity::find()
            .all(&args.db_reader)
            .await
            .inspect_err(|e| tracing::error!(error = %e, "failed to query things"))?;

        tracing::debug!(
            toc_entries = toc_rows.len(),
            things = thing_rows.len(),
            "restoring toc"
        );

        let things_map: HashMap<ThingId, ThingInfo> = thing_rows
            .into_iter()
            .map(|t| {
                (
                    ThingId::from(t.id),
                    ThingInfo {
                        name: t.name,
                        status: t.status.into(),
                    },
                )
            })
            .collect();

        let known_things: HashSet<ThingId> = things_map.keys().cloned().collect();
        let (doc, id_map, dirty) = restore_toc(toc_rows, &things_map);
        let doc_room = room::Room::new(doc);

        tracing::debug!(
            tree_size = id_map.len(),
            known_things = known_things.len(),
            dirty,
            "toc actor started"
        );

        let mut the_self = Self {
            campaign_id: args.campaign_id,
            doc_room,
            id_map,
            known_things,
            db_writer: args.db_writer,
            self_ref: actor_ref,
            dirty,
            debounce_duration: args.debounce_duration,
            persist_timer: None,
            fragmenter: BatchFragmenter::new(250 * 1024),
        };
        if dirty {
            tracing::info!("toc restored with orphans, marked dirty");
            the_self.schedule_persist();
        }
        Ok(the_self)
    }

    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        _reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        let span = tracing::info_span!("toc_actor", campaign_id = %self.campaign_id.0);
        let _guard = span.enter();

        if self.dirty {
            if let Err(err) = self.persist_now().await {
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

    async fn handle(
        &mut self,
        msg: room_actor::ClientJoin,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        use familiar_systems_app_shared::campaigns::internal::CampaignRole;
        // TODO: per-Thing write access for players. When ThingActor lands,
        // the mapping here should check Thing-level permissions, not just
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

    async fn handle(
        &mut self,
        msg: room_actor::ClientUpdate,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let old_version = self.doc_room.doc().version();
        let (broadcast, ack) = self.doc_room.apply_updates(msg.client, &msg.updates)?;
        if old_version != ack.version {
            self.dirty = true;
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
// PersistNow
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct PersistNow;

impl TocActor {
    fn schedule_persist(&mut self) {
        if let Some(handle) = self.persist_timer.take() {
            handle.abort();
        }
        let self_ref = self.self_ref.clone();
        let duration = self.debounce_duration;
        self.persist_timer = Some(tokio::spawn(async move {
            tokio::time::sleep(duration).await;
            let _ = self_ref.tell(PersistNow).send().await;
        }))
    }

    async fn persist_now(&mut self) -> Result<(), sea_orm::DbErr> {
        let rows = snapshot_toc(self.doc_room.doc(), &mut self.id_map, &self.known_things);
        let row_count = rows.len();
        tracing::debug!("Persisting TOC snapshot: {} rows", row_count);

        if let Err(err) = self.db_writer.tell(WriteTocSnapshot { rows }).await {
            tracing::error!(error=%err, "Failed to send toc snapshot to database");
        } else {
            tracing::debug!(row_count, "toc snapshot enqueued for write")
        }

        self.dirty = false;
        Ok(())
    }
}

impl Message<PersistNow> for TocActor {
    type Reply = Result<(), sea_orm::DbErr>;

    async fn handle(
        &mut self,
        _: PersistNow,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        if self.dirty {
            self.persist_now().await?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Persistence: snapshot & restore
// ---------------------------------------------------------------------------

/// Info about a Thing needed during ToC restore (title and visibility).
pub struct ThingInfo {
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
/// Skips `TocEntry::Suggestion` entries (deferred) and `TocEntry::Thing`
/// entries whose `thing_id` is not in `known_things` (dangling references).
pub fn snapshot_toc(
    doc: &LoroTocDoc,
    id_map: &mut HashMap<TreeID, String>,
    known_things: &HashSet<ThingId>,
) -> Vec<toc_entries::ActiveModel> {
    let tree = doc.read_tree();
    let mut rows = Vec::new();
    collect_rows(&tree, None, id_map, known_things, &mut rows);
    rows
}

fn collect_rows(
    nodes: &[TocTreeNode],
    parent_row_id: Option<String>,
    id_map: &mut HashMap<TreeID, String>,
    known_things: &HashSet<ThingId>,
    out: &mut Vec<toc_entries::ActiveModel>,
) {
    let mut position: i32 = 0;
    for node in nodes {
        let (thing_id, folder_title) = match &node.entry {
            TocEntry::Folder { title, .. } => (None, Some(title.clone())),
            TocEntry::Thing { thing_id, .. } => {
                if !known_things.contains(thing_id) {
                    continue;
                }
                (Some(ThingIdCol::from(thing_id.clone())), None)
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
            thing_id: Set(thing_id),
            folder_title: Set(folder_title),
            visibility: Set(visibility),
            parent_id: Set(parent_row_id.clone()),
            position: Set(position),
        });
        position += 1;

        collect_rows(&node.children, Some(row_id), id_map, known_things, out);
    }
}

/// Rebuild a `LoroTocDoc` from persisted rows and the current Things in the DB.
///
/// Entries are inserted in topological order (parents before children, siblings
/// by position). Thing titles come from the `things` map, not from the
/// toc_entries table, so they're always fresh.
///
/// Returns `(doc, id_map, dirty)`:
/// - `id_map`: the `TreeID -> row ID` mapping for future snapshots.
/// - `dirty`: true when orphan Things were appended (caller should writeback).
///
/// Any Thing present in `things` but not referenced by any toc_entry is an
/// orphan: appended to root with its own visibility.
pub fn restore_toc(
    entries: Vec<toc_entries::Model>,
    things: &HashMap<ThingId, ThingInfo>,
) -> (LoroTocDoc, HashMap<TreeID, String>, bool) {
    let mut doc = LoroTocDoc::new();
    let sorted = topological_sort(entries);

    // row_id -> TreeID for parent lookups during restore
    let mut row_to_tree: HashMap<String, TreeID> = HashMap::new();
    // TreeID -> row_id returned for future snapshots
    let mut tree_to_row: HashMap<TreeID, String> = HashMap::new();

    let mut referenced_things: HashSet<ThingId> = HashSet::new();

    for entry in &sorted {
        let parent_tree_id = entry
            .parent_id
            .as_ref()
            .and_then(|pid| row_to_tree.get(pid))
            .copied();

        let toc_entry = match row_to_entry(entry, things) {
            Some(e) => e,
            None => continue,
        };

        if let Some(ref tid) = entry.thing_id {
            referenced_things.insert(ThingId::from(tid.clone()));
        }

        let (_, tree_id) = doc
            .add_entry(parent_tree_id, &toc_entry)
            .expect("add entry during restore");
        row_to_tree.insert(entry.id.clone(), tree_id);
        tree_to_row.insert(tree_id, entry.id.clone());
    }

    let mut dirty = false;
    for (thing_id, info) in things {
        if !referenced_things.contains(thing_id) {
            let entry = TocEntry::Thing {
                title: info.name.clone(),
                thing_id: thing_id.clone(),
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
/// Returns `None` for Thing entries whose thing_id is missing from the map
/// (deleted between writeback and restore).
fn row_to_entry(
    row: &toc_entries::Model,
    things: &HashMap<ThingId, ThingInfo>,
) -> Option<TocEntry> {
    let visibility: Status = row.visibility.into();

    if let Some(ref thing_id_col) = row.thing_id {
        let thing_id = ThingId::from(thing_id_col.clone());
        let info = things.get(&thing_id)?;
        Some(TocEntry::Thing {
            title: info.name.clone(),
            thing_id,
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
    use familiar_systems_campaign_shared::loro::toc::TocEntryKind;

    fn make_folder(title: &str) -> TocEntry {
        TocEntry::Folder {
            title: title.to_string(),
            visibility: Status::Known,
            suggestions: Vec::new(),
        }
    }

    fn make_thing(title: &str) -> (TocEntry, ThingId) {
        let thing_id = ThingId::generate();
        let entry = TocEntry::Thing {
            title: title.to_string(),
            thing_id: thing_id.clone(),
            visibility: Status::Known,
            suggestions: Vec::new(),
        };
        (entry, thing_id)
    }

    fn active_to_model(am: toc_entries::ActiveModel) -> toc_entries::Model {
        toc_entries::Model {
            id: am.id.unwrap(),
            thing_id: am.thing_id.unwrap(),
            folder_title: am.folder_title.unwrap(),
            visibility: am.visibility.unwrap(),
            parent_id: am.parent_id.unwrap(),
            position: am.position.unwrap(),
        }
    }

    fn thing_info(name: &str, status: Status) -> ThingInfo {
        ThingInfo {
            name: name.to_string(),
            status,
        }
    }

    fn things_map(items: &[(ThingId, &str, Status)]) -> HashMap<ThingId, ThingInfo> {
        items
            .iter()
            .map(|(id, name, status)| (id.clone(), thing_info(name, *status)))
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

        let (thing_entry, thing_id) = make_thing("The Iron Citadel");
        doc.add_entry(None, &thing_entry).unwrap();

        let things = things_map(&[(thing_id.clone(), "The Iron Citadel", Status::Known)]);
        let known: HashSet<ThingId> = things.keys().cloned().collect();

        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &known);
        assert_eq!(rows.len(), 2);

        let models: Vec<_> = rows.into_iter().map(active_to_model).collect();
        let (restored, _, dirty) = restore_toc(models, &things);
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

        let (child1, tid1) = make_thing("The Dragon's Lair");
        doc.add_entry(Some(folder_id), &child1).unwrap();

        let (child2, tid2) = make_thing("The Crystal Cave");
        let (_, child2_id) = doc.add_entry(Some(folder_id), &child2).unwrap();

        let (grandchild, tid3) = make_thing("The Hidden Alcove");
        doc.add_entry(Some(child2_id), &grandchild).unwrap();

        let things = things_map(&[
            (tid1, "The Dragon's Lair", Status::Known),
            (tid2, "The Crystal Cave", Status::Known),
            (tid3, "The Hidden Alcove", Status::Known),
        ]);
        let known: HashSet<ThingId> = things.keys().cloned().collect();

        let mut id_map = HashMap::new();
        let rows = snapshot_toc(&doc, &mut id_map, &known);
        assert_eq!(rows.len(), 4);

        let models: Vec<_> = rows.into_iter().map(active_to_model).collect();
        let (restored, _, dirty) = restore_toc(models, &things);
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

    // -- Dangling thing_id test --

    #[test]
    fn snapshot_drops_dangling_thing_entries() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(None, &make_folder("Keeper")).unwrap();

        let (dangling, _) = make_thing("Ghost Entry");
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
        let (thing_a, tid_a) = make_thing("Alpha");
        let (thing_b, _) = make_thing("Dangling Beta");
        let (thing_c, tid_c) = make_thing("Gamma");
        doc.add_entry(None, &thing_a).unwrap();
        doc.add_entry(None, &thing_b).unwrap();
        doc.add_entry(None, &thing_c).unwrap();

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
    fn orphan_things_appended_to_root() {
        let orphan_id = ThingId::generate();
        let things = things_map(&[(orphan_id.clone(), "Lost Artifact", Status::GmOnly)]);

        let (doc, id_map, dirty) = restore_toc(Vec::new(), &things);
        assert!(dirty);
        assert_eq!(id_map.len(), 1, "orphan gets an entry in the id_map");

        let tree = doc.read_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry.kind(), TocEntryKind::Thing);
        assert_eq!(tree[0].entry.title(), Some("Lost Artifact"));
        assert_eq!(*tree[0].entry.visibility(), Status::GmOnly);
    }

    #[test]
    fn orphan_inherits_thing_visibility() {
        let id = ThingId::generate();
        let things = things_map(&[(id.clone(), "Public NPC", Status::Known)]);

        let (doc, _, _) = restore_toc(Vec::new(), &things);
        assert_eq!(*doc.read_tree()[0].entry.visibility(), Status::Known);
    }

    // -- Restore edge cases --

    #[test]
    fn restore_skips_entries_with_deleted_things() {
        let stale_thing_id = ThingIdCol::from(ThingId::generate());
        let row = toc_entries::Model {
            id: ulid::Ulid::new().to_string(),
            thing_id: Some(stale_thing_id),
            folder_title: None,
            visibility: StatusCol::Known,
            parent_id: None,
            position: 0,
        };

        let (doc, id_map, dirty) = restore_toc(vec![row], &HashMap::new());
        assert!(doc.read_tree().is_empty(), "stale entry is skipped");
        assert!(id_map.is_empty(), "skipped entry has no id_map entry");
        assert!(!dirty, "no orphans because the thing is gone entirely");
    }

    #[test]
    fn topological_sort_handles_out_of_order_input() {
        let parent_id = ulid::Ulid::new().to_string();
        let child_id = ulid::Ulid::new().to_string();

        let child = toc_entries::Model {
            id: child_id.clone(),
            thing_id: None,
            folder_title: Some("Child".to_string()),
            visibility: StatusCol::Known,
            parent_id: Some(parent_id.clone()),
            position: 0,
        };
        let parent = toc_entries::Model {
            id: parent_id.clone(),
            thing_id: None,
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
            thing_id: None,
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
        let (thing_entry, tid) = make_thing("Also Stable");
        doc.add_entry(None, &thing_entry).unwrap();

        let things = things_map(&[(tid.clone(), "Also Stable", Status::Known)]);
        let known: HashSet<ThingId> = things.keys().cloned().collect();

        // First snapshot: generates fresh IDs.
        let mut id_map = HashMap::new();
        let rows1 = snapshot_toc(&doc, &mut id_map, &known);
        let ids1 = row_ids(&rows1);
        assert_eq!(ids1.len(), 2);

        // Restore from those rows, get a new id_map.
        let models: Vec<_> = rows1.into_iter().map(active_to_model).collect();
        let (restored, mut restored_id_map, _) = restore_toc(models, &things);

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
}
