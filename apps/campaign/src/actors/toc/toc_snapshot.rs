//! Pure serialization between the ToC `LoroTree` and persisted `toc_entries`
//! rows. No actor state, no I/O: `snapshot_toc` walks the live tree into rows,
//! `restore_toc` rebuilds the tree from rows + the current Pages. Lives beside
//! `toc_actor` (which calls both) so the topo-sort / orphan / stable-id logic is
//! testable without spawning an actor.

use std::collections::{HashMap, HashSet};

use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::loro::toc::{TocEntry, TocPageKind};
use familiar_systems_campaign_shared::status::Status;
use loro::TreeID;
use sea_orm::ActiveValue::Set;

use crate::entities::columns::{PageIdCol, StatusCol};
use crate::entities::toc_entries;
use crate::loro::toc::{LoroTocDoc, TocTreeNode};

// ---------------------------------------------------------------------------
// Persistence: snapshot & restore
// ---------------------------------------------------------------------------

/// Info about a Page needed during ToC restore: the denormalized fields the ToC
/// node carries (title, visibility) plus the `page_kind` sum the client uses to
/// compose the display name (a session's ordinal lives inside that sum).
pub struct PageInfo {
    pub name: String,
    pub status: Status,
    pub page_kind: TocPageKind,
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
                page_kind: info.page_kind.clone(),
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
            page_kind: info.page_kind.clone(),
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

    fn make_page(title: &str) -> (TocEntry, PageId) {
        let page_id = PageId::generate();
        let entry = TocEntry::Page {
            title: title.to_string(),
            page_id: page_id.clone(),
            page_kind: TocPageKind::Entity,
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
            page_kind: TocPageKind::Entity,
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

    #[test]
    fn restored_session_entry_carries_kind_and_ordinal() {
        // A session page surfaces as a Page entry that carries its kind and
        // ordinal, so the client can render "Session {ordinal}: {name}" without
        // querying the temporal table.
        let id = PageId::generate();
        let mut pages = HashMap::new();
        pages.insert(
            id.clone(),
            PageInfo {
                name: "The Gathering Storm".to_string(),
                status: Status::GmOnly,
                page_kind: TocPageKind::Session { ordinal: 7 },
            },
        );

        let (doc, _, _) = restore_toc(Vec::new(), &pages);
        let tree = doc.read_tree();
        match &tree[0].entry {
            TocEntry::Page {
                page_kind, title, ..
            } => {
                assert_eq!(*page_kind, TocPageKind::Session { ordinal: 7 });
                assert_eq!(
                    title, "The Gathering Storm",
                    "the session's name carries through restore"
                );
            }
            other => panic!("expected a session Page entry, got {other:?}"),
        }
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
}
