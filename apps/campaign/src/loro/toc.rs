//! Typed Loro wrapper for the Table of Contents.
//!
//! The ToC is a collaborative, nested list of entries backed by `LoroTree`.
//! Schema constants (container names, key names) and the ts-rs-exported
//! `TocEntry`/`TocEntryKind` domain types live in
//! `familiar_systems_campaign_shared::loro::toc`. The wrapper below is the
//! Rust-side read/write interface against that schema.

use serde::Serialize;
use std::borrow::Cow;

use familiar_systems_campaign_shared::id::{ConversationId, ThingId};
use familiar_systems_campaign_shared::loro::toc::{
    CONTAINER_TOC, KEY_CONVERSATION_ID, KEY_KIND, KEY_THING_ID, KEY_TITLE, KEY_VISIBILITY,
    KIND_FOLDER, KIND_SUGGESTION, KIND_THING, TocEntry,
};
use familiar_systems_campaign_shared::status::Status;
use loro::{LoroDoc, LoroMap, LoroTree, LoroValue, TreeID, ValueOrContainer};

use crate::domain::crdt::doc::{CrdtDoc, DocError, Snapshot, VersionVector};

/// A node in the ToC tree, as read from the LoroDoc.
/// Contains the entry data and its children (recursive).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TocTreeNode {
    pub tree_id: TreeID,
    pub entry: TocEntry,
    pub children: Vec<TocTreeNode>,
}

/// Typed wrapper around a `LoroDoc` for the Table of Contents.
///
/// ## Loro container layout
///
/// ```text
/// LoroDoc root:
///   "toc" (LoroTree, fractional index enabled)
///     Node metadata (LoroMap per node):
///       "kind": "folder" | "thing" | "suggestion"
///       "title": string
///       "visibility": "gmOnly" | "known" | "retconned"
///       "thingId": string          (Thing only)
///       "conversationId": string   (Suggestion only)
/// ```
pub struct LoroTocDoc {
    doc: LoroDoc,
}

#[allow(clippy::new_without_default)]
impl LoroTocDoc {
    /// Create a new empty ToC document with its tree container initialized.
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        // Initialize the tree up front so it exists from the start, avoiding the
        // concurrent insert_container hazard.
        let tree = doc.get_tree(CONTAINER_TOC);
        tree.enable_fractional_index(0);
        Self { doc }
    }

    /// Restore from a snapshot blob.
    pub fn from_snapshot(snapshot: &Snapshot) -> Result<Self, DocError> {
        let doc = LoroDoc::new();
        doc.import(snapshot.as_bytes())
            .map_err(|e| DocError::ImportSnapshot(e.to_string()))?;
        Ok(Self { doc })
    }

    // -- Private helpers --

    fn tree(&self) -> LoroTree {
        self.doc.get_tree(CONTAINER_TOC)
    }

    /// Capture version vector, run mutation, export the delta for broadcasting.
    fn with_delta(&self, f: impl FnOnce()) -> Result<Vec<u8>, String> {
        let vv_before = self.doc.oplog_vv();
        f();
        self.doc
            .export(loro::ExportMode::Updates {
                from: Cow::Owned(vv_before),
            })
            .map_err(|e| format!("failed to export toc update: {e}"))
    }

    fn status_to_str(s: &Status) -> &'static str {
        match s {
            Status::GmOnly => "gmOnly",
            Status::Known => "known",
            Status::Retconned => "retconned",
        }
    }

    fn str_to_status(s: &str) -> Option<Status> {
        match s {
            "gmOnly" => Some(Status::GmOnly),
            "known" => Some(Status::Known),
            "retconned" => Some(Status::Retconned),
            _ => None,
        }
    }

    fn read_status(meta: &LoroMap) -> Option<Status> {
        match meta.get(KEY_VISIBILITY)? {
            ValueOrContainer::Value(LoroValue::String(s)) => Self::str_to_status(&s),
            _ => None,
        }
    }

    /// Write a `TocEntry` into a tree node's metadata map.
    ///
    /// TODO: Inline suggestions (`TocSuggestion`) on Folder/Thing entries are
    /// not written here. They need their own read/write path, likely as a
    /// LoroList sub-container on each node's metadata map. During an active
    /// session the CRDT is authoritative for suggestion state; on checkout
    /// they are hydrated from SQLite by the TocActor.
    fn write_entry_to_meta(meta: &LoroMap, entry: &TocEntry) {
        match entry {
            TocEntry::Folder {
                title, visibility, ..
            } => {
                meta.insert(KEY_KIND, KIND_FOLDER).unwrap();
                meta.insert(KEY_TITLE, title.as_str()).unwrap();
                meta.insert(KEY_VISIBILITY, Self::status_to_str(visibility))
                    .unwrap();
            }
            TocEntry::Thing {
                title,
                thing_id,
                visibility,
                ..
            } => {
                meta.insert(KEY_KIND, KIND_THING).unwrap();
                meta.insert(KEY_TITLE, title.as_str()).unwrap();
                meta.insert(KEY_THING_ID, thing_id.0.to_string()).unwrap();
                meta.insert(KEY_VISIBILITY, Self::status_to_str(visibility))
                    .unwrap();
            }
            TocEntry::Suggestion {
                conversation_id,
                title,
                visibility,
            } => {
                meta.insert(KEY_KIND, KIND_SUGGESTION).unwrap();
                if let Some(t) = title {
                    meta.insert(KEY_TITLE, t.as_str()).unwrap();
                }
                meta.insert(KEY_CONVERSATION_ID, conversation_id.0.to_string().as_str())
                    .unwrap();
                meta.insert(KEY_VISIBILITY, Self::status_to_str(visibility))
                    .unwrap();
            }
        }
    }

    /// Read a `TocEntry` from a tree node's metadata map.
    /// Inline suggestions are not read here (see TODO on `write_entry_to_meta`).
    fn read_entry_from_meta(meta: &LoroMap) -> Option<TocEntry> {
        let kind = match meta.get(KEY_KIND)? {
            ValueOrContainer::Value(LoroValue::String(s)) => s.to_string(),
            _ => return None,
        };
        let visibility = Self::read_status(meta)?;
        match kind.as_str() {
            KIND_FOLDER => {
                let title = match meta.get(KEY_TITLE)? {
                    ValueOrContainer::Value(LoroValue::String(s)) => s.to_string(),
                    _ => return None,
                };
                Some(TocEntry::Folder {
                    title,
                    visibility,
                    suggestions: Vec::new(),
                })
            }
            KIND_THING => {
                let title = match meta.get(KEY_TITLE)? {
                    ValueOrContainer::Value(LoroValue::String(s)) => s.to_string(),
                    _ => return None,
                };
                let thing_id = match meta.get(KEY_THING_ID)? {
                    ValueOrContainer::Value(LoroValue::String(s)) => {
                        ThingId::from(ulid::Ulid::from_string(&s).ok()?)
                    }
                    _ => return None,
                };
                Some(TocEntry::Thing {
                    title,
                    thing_id,
                    visibility,
                    suggestions: Vec::new(),
                })
            }
            KIND_SUGGESTION => {
                let title = match meta.get(KEY_TITLE) {
                    Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
                    _ => None,
                };
                let conversation_id = match meta.get(KEY_CONVERSATION_ID)? {
                    ValueOrContainer::Value(LoroValue::String(s)) => {
                        ConversationId(ulid::Ulid::from_string(&s).ok()?)
                    }
                    _ => return None,
                };
                Some(TocEntry::Suggestion {
                    conversation_id,
                    title,
                    visibility,
                })
            }
            _ => None,
        }
    }

    /// Recursively build a `TocTreeNode` tree for the given parent.
    fn read_children(&self, tree: &LoroTree, parent_id: Option<TreeID>) -> Vec<TocTreeNode> {
        let child_ids = match parent_id {
            Some(id) => tree.children(id),
            None => tree.children(loro::TreeParentId::Root),
        };

        let Some(child_ids) = child_ids else {
            return Vec::new();
        };

        child_ids
            .into_iter()
            .filter_map(|tree_id| {
                let meta = tree.get_meta(tree_id).ok()?;
                let entry = Self::read_entry_from_meta(&meta)?;
                let children = self.read_children(tree, Some(tree_id));
                Some(TocTreeNode {
                    tree_id,
                    entry,
                    children,
                })
            })
            .collect()
    }

    // -- Public domain methods --

    /// Add an entry to the ToC. Appends as the last child of `parent`
    /// (or at root level if `parent` is `None`).
    /// Returns the delta bytes for broadcasting and the new node's TreeID.
    pub fn add_entry(
        &mut self,
        parent: Option<TreeID>,
        entry: &TocEntry,
    ) -> Result<(Vec<u8>, TreeID), String> {
        let tree = self.tree();
        let mut new_id = None;
        let delta = self.with_delta(|| {
            let id = tree.create(parent).expect("create tree node");
            let meta = tree.get_meta(id).expect("get node meta");
            Self::write_entry_to_meta(&meta, entry);
            new_id = Some(id);
        })?;
        Ok((delta, new_id.expect("node was created")))
    }

    /// Move a node to a new parent (or root if `None`), appended as the last child.
    /// Returns the delta bytes for broadcasting.
    pub fn move_entry(
        &mut self,
        node: TreeID,
        new_parent: Option<TreeID>,
    ) -> Result<Vec<u8>, String> {
        let tree = self.tree();
        self.with_delta(|| {
            tree.mov(node, new_parent).expect("move tree node");
        })
    }

    /// Move a node to appear before `before` (same parent level).
    /// Returns the delta bytes for broadcasting.
    pub fn move_before(&mut self, node: TreeID, before: TreeID) -> Result<Vec<u8>, String> {
        let tree = self.tree();
        self.with_delta(|| {
            tree.mov_before(node, before).expect("move_before");
        })
    }

    /// Remove an entry from the ToC. Returns the delta bytes for broadcasting.
    pub fn remove_entry(&mut self, node: TreeID) -> Result<Vec<u8>, String> {
        let tree = self.tree();
        self.with_delta(|| {
            tree.delete(node).expect("delete tree node");
        })
    }

    /// Update the metadata of an existing entry.
    /// Returns the delta bytes for broadcasting.
    pub fn update_entry(&mut self, node: TreeID, entry: &TocEntry) -> Result<Vec<u8>, String> {
        let tree = self.tree();
        self.with_delta(|| {
            let meta = tree.get_meta(node).expect("get node meta");
            Self::write_entry_to_meta(&meta, entry);
        })
    }

    /// Read the full ToC tree structure.
    pub fn read_tree(&self) -> Vec<TocTreeNode> {
        let tree = self.tree();
        self.read_children(&tree, None)
    }

    /// Find the `TreeID` of the node representing `thing_id`, if it appears
    /// anywhere in the tree. Used to resolve a parent Thing for placement.
    pub fn find_thing_node(&self, thing_id: &ThingId) -> Option<TreeID> {
        fn search(nodes: &[TocTreeNode], target: &ThingId) -> Option<TreeID> {
            for node in nodes {
                if let TocEntry::Thing { thing_id: id, .. } = &node.entry
                    && id == target
                {
                    return Some(node.tree_id);
                }
                if let Some(found) = search(&node.children, target) {
                    return Some(found);
                }
            }
            None
        }
        search(&self.read_tree(), thing_id)
    }

    /// Read a single entry by TreeID.
    pub fn read_entry(&self, node: TreeID) -> Option<TocEntry> {
        let tree = self.tree();
        let meta = tree.get_meta(node).ok()?;
        Self::read_entry_from_meta(&meta)
    }
}

impl CrdtDoc for LoroTocDoc {
    fn version(&self) -> VersionVector {
        VersionVector(self.doc.oplog_vv().encode())
    }

    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), DocError> {
        for (i, update) in updates.iter().enumerate() {
            self.doc.import(update).map_err(|e| {
                DocError::ApplyUpdate(format!("toc update {i} ({} bytes): {e}", update.len()))
            })?;
        }
        Ok(())
    }

    fn export_snapshot(&self) -> Result<Snapshot, DocError> {
        self.doc
            .export(loro::ExportMode::Snapshot)
            .map(Snapshot)
            .map_err(|e| DocError::ExportSnapshot(e.to_string()))
    }

    fn import_snapshot(&mut self, data: &Snapshot) -> Result<(), DocError> {
        self.doc
            .import(data.as_bytes())
            .map(|_| ())
            .map_err(|e| DocError::ImportSnapshot(e.to_string()))
    }

    fn debug_value(&self) -> Option<serde_json::Value> {
        Some(self.doc.get_deep_value().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use familiar_systems_campaign_shared::loro::toc::TocEntryKind;

    fn folder(title: &str) -> TocEntry {
        TocEntry::Folder {
            title: title.to_string(),
            visibility: Status::Known,
            suggestions: Vec::new(),
        }
    }

    fn thing(title: &str) -> (TocEntry, ThingId) {
        let thing_id = ThingId::generate();
        let entry = TocEntry::Thing {
            title: title.to_string(),
            thing_id: thing_id.clone(),
            visibility: Status::Known,
            suggestions: Vec::new(),
        };
        (entry, thing_id)
    }

    #[test]
    fn new_toc_has_empty_tree() {
        let doc = LoroTocDoc::new();
        let tree = doc.read_tree();
        assert!(tree.is_empty());
    }

    #[test]
    fn add_folder_at_root() {
        let mut doc = LoroTocDoc::new();
        let entry = folder("Chapter 1");
        let (delta, _id) = doc.add_entry(None, &entry).unwrap();
        assert!(!delta.is_empty());

        let tree = doc.read_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry, entry);
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn add_thing_entry() {
        let mut doc = LoroTocDoc::new();
        let (entry, expected_id) = thing("Korgath the Destroyer");
        let (_, _) = doc.add_entry(None, &entry).unwrap();

        let tree = doc.read_tree();
        assert_eq!(tree[0].entry.kind(), TocEntryKind::Thing);
        if let TocEntry::Thing { thing_id, .. } = &tree[0].entry {
            assert_eq!(*thing_id, expected_id);
        } else {
            panic!("expected Thing variant");
        }
    }

    #[test]
    fn add_suggestion_entry() {
        let mut doc = LoroTocDoc::new();
        let entry = TocEntry::Suggestion {
            conversation_id: ConversationId(ulid::Ulid::new()),
            title: Some("Proposed NPC".to_string()),
            visibility: Status::GmOnly,
        };
        let (_, _) = doc.add_entry(None, &entry).unwrap();

        let tree = doc.read_tree();
        assert_eq!(tree[0].entry.kind(), TocEntryKind::Suggestion);
        assert_eq!(tree[0].entry.title(), Some("Proposed NPC"));
    }

    #[test]
    fn suggestion_without_title() {
        let mut doc = LoroTocDoc::new();
        let entry = TocEntry::Suggestion {
            conversation_id: ConversationId(ulid::Ulid::new()),
            title: None,
            visibility: Status::GmOnly,
        };
        let (_, _) = doc.add_entry(None, &entry).unwrap();

        let tree = doc.read_tree();
        assert_eq!(tree[0].entry.title(), None);
    }

    #[test]
    fn visibility_round_trips() {
        let mut doc = LoroTocDoc::new();
        for status in [Status::GmOnly, Status::Known, Status::Retconned] {
            let entry = TocEntry::Folder {
                title: format!("{status:?}"),
                visibility: status,
                suggestions: Vec::new(),
            };
            doc.add_entry(None, &entry).unwrap();
        }

        let tree = doc.read_tree();
        assert_eq!(tree[0].entry.visibility(), &Status::GmOnly);
        assert_eq!(tree[1].entry.visibility(), &Status::Known);
        assert_eq!(tree[2].entry.visibility(), &Status::Retconned);
    }

    #[test]
    fn nested_entries() {
        let mut doc = LoroTocDoc::new();
        let (_, parent_id) = doc.add_entry(None, &folder("Act I")).unwrap();
        let (child_entry, _) = thing("The Dragon's Lair");
        doc.add_entry(Some(parent_id), &child_entry).unwrap();

        let tree = doc.read_tree();
        assert_eq!(tree.len(), 1, "one root node");
        assert_eq!(tree[0].children.len(), 1, "one child");
        assert_eq!(tree[0].children[0].entry, child_entry);
    }

    #[test]
    fn find_thing_node_locates_nested_thing() {
        let mut doc = LoroTocDoc::new();
        let (_, folder_id) = doc.add_entry(None, &folder("Act I")).unwrap();
        let (thing_entry, thing_id) = thing("The Dragon's Lair");
        let (_, tree_id) = doc.add_entry(Some(folder_id), &thing_entry).unwrap();

        assert_eq!(doc.find_thing_node(&thing_id), Some(tree_id));
        assert!(
            doc.find_thing_node(&ThingId::generate()).is_none(),
            "absent thing resolves to None"
        );
    }

    #[test]
    fn remove_entry() {
        let mut doc = LoroTocDoc::new();
        let (_, id) = doc.add_entry(None, &folder("Temp")).unwrap();

        let delta = doc.remove_entry(id).unwrap();
        assert!(!delta.is_empty());

        let tree = doc.read_tree();
        assert!(tree.is_empty());
    }

    #[test]
    fn update_entry() {
        let mut doc = LoroTocDoc::new();
        let (_, id) = doc.add_entry(None, &folder("Draft")).unwrap();

        let (updated, _) = thing("Final");
        doc.update_entry(id, &updated).unwrap();

        let entry = doc.read_entry(id).unwrap();
        assert_eq!(entry, updated);
    }

    #[test]
    fn snapshot_round_trip() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(None, &folder("A")).unwrap();

        let snapshot = doc.export_snapshot().unwrap();
        let doc2 = LoroTocDoc::from_snapshot(&snapshot).unwrap();

        let tree = doc2.read_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry.title(), Some("A"));
    }

    /// Proves that a client doc stays converged with a server doc across
    /// a realistic sequence of mutations, each applied as a broadcast delta.
    /// This is the contract the TocActor relies on: every public mutation
    /// returns a delta, and applying that delta to a clone reproduces the
    /// full state.
    #[test]
    fn server_client_convergence_across_mutation_sequence() {
        macro_rules! apply {
            ($server:expr, $client:expr, $delta:expr) => {{
                $client.apply_updates(&[$delta]).unwrap();
                assert_eq!(
                    $server.debug_value(),
                    $client.debug_value(),
                    "docs diverged after applying delta"
                );
            }};
        }

        let mut server = LoroTocDoc::new();
        let snapshot = server.export_snapshot().unwrap();
        let mut client = LoroTocDoc::from_snapshot(&snapshot).unwrap();

        // 1. Add two root-level entries: a folder and a thing.
        let (delta, folder_id) = server.add_entry(None, &folder("Act I")).unwrap();
        apply!(server, client, delta);

        let (thing_entry, _thing_id) = thing("The Iron Citadel");
        let (delta, thing_node_id) = server.add_entry(None, &thing_entry).unwrap();
        apply!(server, client, delta);

        assert_eq!(server.read_tree().len(), 2);

        // 2. Move the thing under the folder.
        let delta = server.move_entry(thing_node_id, Some(folder_id)).unwrap();
        apply!(server, client, delta);

        let tree = server.read_tree();
        assert_eq!(tree.len(), 1, "only the folder at root");
        assert_eq!(tree[0].children.len(), 1, "thing is now a child");
        assert_eq!(tree[0].children[0].entry, thing_entry);

        // 3. Add a second child and reorder it before the first.
        let (sibling_entry, _) = thing("The Shattered Gate");
        let (delta, sibling_id) = server.add_entry(Some(folder_id), &sibling_entry).unwrap();
        apply!(server, client, delta);

        let delta = server.move_before(sibling_id, thing_node_id).unwrap();
        apply!(server, client, delta);

        let children = &server.read_tree()[0].children;
        assert_eq!(children[0].entry, sibling_entry, "sibling moved first");
        assert_eq!(children[1].entry, thing_entry, "original thing second");

        // 4. Update an entry's metadata.
        let updated = TocEntry::Folder {
            title: "Act I: The Beginning".to_string(),
            visibility: Status::GmOnly,
            suggestions: Vec::new(),
        };
        let delta = server.update_entry(folder_id, &updated).unwrap();
        apply!(server, client, delta);

        assert_eq!(server.read_entry(folder_id).unwrap(), updated);

        // 5. Remove an entry.
        let delta = server.remove_entry(sibling_id).unwrap();
        apply!(server, client, delta);

        let tree = server.read_tree();
        assert_eq!(tree[0].children.len(), 1, "sibling removed");

        // Final: full read_tree equality (not just debug_value).
        assert_eq!(server.read_tree(), client.read_tree());
    }
}
