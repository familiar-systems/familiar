//! Table of Contents: schema types, constants, and typed Loro wrapper.
//!
//! The ToC is a collaborative, nested list of entries backed by `LoroTree`. Each
//! entry is either plain text (a heading/divider), a link to a Thing, or a link
//! to a Journal. Entries can be nested up to a configurable max depth (1-4).
//!
//! This is a **non-ProseMirror** CRDT structure: both sides (Rust and TypeScript)
//! access the LoroDoc directly through typed wrappers, rather than going through
//! `loro-prosemirror`.

use std::borrow::Cow;

use loro::{LoroDoc, LoroMap, LoroTree, LoroValue, TreeID, ValueOrContainer};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{JournalId, ThingId};

use super::{CrdtDoc, Snapshot, VersionVector};

// ── Schema: Loro container names ────────────────────────────────────────────

/// Top-level LoroTree container for the table of contents.
pub const CONTAINER_TOC: &str = "toc";

/// Top-level LoroMap container for ToC metadata.
pub const CONTAINER_META: &str = "meta";

// ── Schema: LoroMap key constants ───────────────────────────────────────────

pub const KEY_KIND: &str = "kind";
pub const KEY_TITLE: &str = "title";
pub const KEY_THING_ID: &str = "thingId";
pub const KEY_JOURNAL_ID: &str = "journalId";
pub const KEY_LANDING_PAGE_ID: &str = "landingPageId";

/// Maximum nesting depth for ToC entries. Ex:
/// - One
/// - - Two
/// - - - Three
pub const MAX_DEPTH: usize = 3;

// ── Schema: kind string values ──────────────────────────────────────────────

pub const KIND_TEXT: &str = "text";
pub const KIND_THING: &str = "thing";
pub const KIND_JOURNAL: &str = "journal";

// ── Schema: domain types ────────────────────────────────────────────────────

/// Discriminant-only enum for filtering and display.
/// Derivable from `TocEntry` via `TocEntry::kind()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum TocEntryKind {
    Text,
    Thing,
    Journal,
}

/// A single ToC entry as a discriminated union.
///
/// Each variant carries only the fields valid for that kind, making invalid states
/// unrepresentable (e.g., a `Text` entry cannot have a `thing_id`).
///
/// **Loro storage** uses variant-specific keys (`thingId`, `journalId`).
/// Conversion between flat LoroMap fields and this enum happens in `LoroTocDoc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum TocEntry {
    #[serde(rename = "text")]
    Text { title: String },
    #[serde(rename = "thing")]
    Thing {
        title: String,
        #[serde(rename = "thingId")]
        thing_id: ThingId,
    },
    #[serde(rename = "journal")]
    Journal {
        title: String,
        #[serde(rename = "journalId")]
        journal_id: JournalId,
    },
}

impl TocEntry {
    pub fn kind(&self) -> TocEntryKind {
        match self {
            Self::Text { .. } => TocEntryKind::Text,
            Self::Thing { .. } => TocEntryKind::Thing,
            Self::Journal { .. } => TocEntryKind::Journal,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Text { title } | Self::Thing { title, .. } | Self::Journal { title, .. } => title,
        }
    }
}

// ── Loro wrapper ────────────────────────────────────────────────────────────

/// A node in the ToC tree, as read from the LoroDoc.
/// Contains the entry data and its children (recursive).
#[derive(Debug, Clone)]
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
///   "meta" (LoroMap)
///     "landingPageId": string
///   "toc" (LoroTree, fractional index enabled)
///     Node metadata (LoroMap per node):
///       "kind": "text" | "thing" | "journal"
///       "title": string
///       "thingId" | "journalId": string (variant-specific)
/// ```
pub struct LoroTocDoc {
    doc: LoroDoc,
}

#[allow(clippy::new_without_default)]
impl LoroTocDoc {
    /// Create a new empty ToC document with initialized containers.
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        // Initialize containers so they exist from the start.
        // Avoids the concurrent insert_container hazard.
        let meta = doc.get_map(CONTAINER_META);
        meta.insert(KEY_LANDING_PAGE_ID, "").unwrap();
        let tree = doc.get_tree(CONTAINER_TOC);
        tree.enable_fractional_index(0);
        Self { doc }
    }

    /// Restore from a snapshot blob.
    pub fn from_snapshot(snapshot: &Snapshot) -> Result<Self, String> {
        let doc = LoroDoc::new();
        doc.import(snapshot.as_bytes())
            .map_err(|e| format!("failed to import toc snapshot: {e}"))?;
        Ok(Self { doc })
    }

    // -- Private helpers --

    fn meta(&self) -> LoroMap {
        self.doc.get_map(CONTAINER_META)
    }

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

    /// Write a `TocEntry` into a tree node's metadata map.
    fn write_entry_to_meta(meta: &LoroMap, entry: &TocEntry) {
        match entry {
            TocEntry::Text { title } => {
                meta.insert(KEY_KIND, KIND_TEXT).unwrap();
                meta.insert(KEY_TITLE, title.as_str()).unwrap();
            }
            TocEntry::Thing { title, thing_id } => {
                meta.insert(KEY_KIND, KIND_THING).unwrap();
                meta.insert(KEY_TITLE, title.as_str()).unwrap();
                meta.insert(KEY_THING_ID, thing_id.0.as_str()).unwrap();
            }
            TocEntry::Journal { title, journal_id } => {
                meta.insert(KEY_KIND, KIND_JOURNAL).unwrap();
                meta.insert(KEY_TITLE, title.as_str()).unwrap();
                meta.insert(KEY_JOURNAL_ID, journal_id.0.as_str()).unwrap();
            }
        }
    }

    /// Read a `TocEntry` from a tree node's metadata map.
    fn read_entry_from_meta(meta: &LoroMap) -> Option<TocEntry> {
        let kind = match meta.get(KEY_KIND)? {
            ValueOrContainer::Value(LoroValue::String(s)) => s.to_string(),
            _ => return None,
        };
        let title = match meta.get(KEY_TITLE)? {
            ValueOrContainer::Value(LoroValue::String(s)) => s.to_string(),
            _ => return None,
        };
        match kind.as_str() {
            KIND_TEXT => Some(TocEntry::Text { title }),
            KIND_THING => {
                let thing_id = match meta.get(KEY_THING_ID)? {
                    ValueOrContainer::Value(LoroValue::String(s)) => ThingId(s.to_string()),
                    _ => return None,
                };
                Some(TocEntry::Thing { title, thing_id })
            }
            KIND_JOURNAL => {
                let journal_id = match meta.get(KEY_JOURNAL_ID)? {
                    ValueOrContainer::Value(LoroValue::String(s)) => JournalId(s.to_string()),
                    _ => return None,
                };
                Some(TocEntry::Journal { title, journal_id })
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

    /// Read a single entry by TreeID.
    pub fn read_entry(&self, node: TreeID) -> Option<TocEntry> {
        let tree = self.tree();
        let meta = tree.get_meta(node).ok()?;
        Self::read_entry_from_meta(&meta)
    }

    // -- Metadata methods --

    /// Get the current landing page ID.
    pub fn landing_page_id(&self) -> Option<String> {
        let meta = self.meta();
        match meta.get(KEY_LANDING_PAGE_ID)? {
            ValueOrContainer::Value(LoroValue::String(s)) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    }

    /// Set the landing page ID. Returns delta bytes for broadcasting.
    pub fn set_landing_page(&mut self, page_id: &str) -> Result<Vec<u8>, String> {
        let meta = self.meta();
        self.with_delta(|| {
            meta.insert(KEY_LANDING_PAGE_ID, page_id).unwrap();
        })
    }
}

impl CrdtDoc for LoroTocDoc {
    fn get_version(&self) -> VersionVector {
        VersionVector(self.doc.oplog_vv().encode())
    }

    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), String> {
        for (i, update) in updates.iter().enumerate() {
            self.doc.import(update).map_err(|e| {
                format!(
                    "failed to import toc update {i} ({} bytes): {e}",
                    update.len()
                )
            })?;
        }
        Ok(())
    }

    fn export_snapshot(&self) -> Result<Snapshot, String> {
        self.doc
            .export(loro::ExportMode::Snapshot)
            .map(Snapshot)
            .map_err(|e| format!("failed to export toc snapshot: {e}"))
    }

    fn import_snapshot(&mut self, data: &Snapshot) -> Result<(), String> {
        self.doc
            .import(data.as_bytes())
            .map(|_| ())
            .map_err(|e| format!("failed to import toc snapshot: {e}"))
    }

    fn debug_value(&self) -> Option<serde_json::Value> {
        Some(self.doc.get_deep_value().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_toc_has_empty_tree() {
        let doc = LoroTocDoc::new();
        let tree = doc.read_tree();
        assert!(tree.is_empty());
    }

    #[test]
    fn add_text_entry_at_root() {
        let mut doc = LoroTocDoc::new();
        let entry = TocEntry::Text {
            title: "Chapter 1".to_string(),
        };
        let (delta, _id) = doc.add_entry(None, &entry).unwrap();
        assert!(!delta.is_empty());

        let tree = doc.read_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry, entry);
        assert!(tree[0].children.is_empty());
    }

    #[test]
    fn add_thing_entry_with_target() {
        let mut doc = LoroTocDoc::new();
        let entry = TocEntry::Thing {
            title: "Korgath the Destroyer".to_string(),
            thing_id: ThingId("abc123".to_string()),
        };
        let (_, _) = doc.add_entry(None, &entry).unwrap();

        let tree = doc.read_tree();
        assert_eq!(tree[0].entry.kind(), TocEntryKind::Thing);
        if let TocEntry::Thing { thing_id, .. } = &tree[0].entry {
            assert_eq!(thing_id.0, "abc123");
        } else {
            panic!("expected Thing variant");
        }
    }

    #[test]
    fn add_journal_entry() {
        let mut doc = LoroTocDoc::new();
        let entry = TocEntry::Journal {
            title: "Session 5 Notes".to_string(),
            journal_id: JournalId("j001".to_string()),
        };
        let (_, _) = doc.add_entry(None, &entry).unwrap();

        let tree = doc.read_tree();
        assert_eq!(tree[0].entry.kind(), TocEntryKind::Journal);
    }

    #[test]
    fn nested_entries() {
        let mut doc = LoroTocDoc::new();

        let parent_entry = TocEntry::Text {
            title: "Act I".to_string(),
        };
        let (_, parent_id) = doc.add_entry(None, &parent_entry).unwrap();

        let child_entry = TocEntry::Thing {
            title: "The Dragon's Lair".to_string(),
            thing_id: ThingId("xyz".to_string()),
        };
        let (_, _child_id) = doc.add_entry(Some(parent_id), &child_entry).unwrap();

        let tree = doc.read_tree();
        assert_eq!(tree.len(), 1, "one root node");
        assert_eq!(tree[0].children.len(), 1, "one child");
        assert_eq!(tree[0].children[0].entry, child_entry);
    }

    #[test]
    fn remove_entry() {
        let mut doc = LoroTocDoc::new();
        let (_, id) = doc
            .add_entry(
                None,
                &TocEntry::Text {
                    title: "Temp".to_string(),
                },
            )
            .unwrap();

        let delta = doc.remove_entry(id).unwrap();
        assert!(!delta.is_empty());

        let tree = doc.read_tree();
        assert!(tree.is_empty());
    }

    #[test]
    fn update_entry() {
        let mut doc = LoroTocDoc::new();
        let (_, id) = doc
            .add_entry(
                None,
                &TocEntry::Text {
                    title: "Draft".to_string(),
                },
            )
            .unwrap();

        let updated = TocEntry::Thing {
            title: "Final".to_string(),
            thing_id: ThingId("t1".to_string()),
        };
        doc.update_entry(id, &updated).unwrap();

        let entry = doc.read_entry(id).unwrap();
        assert_eq!(entry, updated);
    }

    #[test]
    fn landing_page() {
        let mut doc = LoroTocDoc::new();
        assert!(doc.landing_page_id().is_none());

        doc.set_landing_page("page1").unwrap();
        assert_eq!(doc.landing_page_id(), Some("page1".to_string()));
    }

    #[test]
    fn snapshot_round_trip() {
        let mut doc = LoroTocDoc::new();
        doc.add_entry(
            None,
            &TocEntry::Text {
                title: "A".to_string(),
            },
        )
        .unwrap();
        doc.set_landing_page("lp1").unwrap();

        let snapshot = doc.export_snapshot().unwrap();
        let doc2 = LoroTocDoc::from_snapshot(&snapshot).unwrap();

        let tree = doc2.read_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry.title(), "A");
        assert_eq!(doc2.landing_page_id(), Some("lp1".to_string()));
    }

    #[test]
    fn delta_can_be_applied_to_another_doc() {
        let mut server = LoroTocDoc::new();
        let snapshot = server.export_snapshot().unwrap();

        let mut client = LoroTocDoc::from_snapshot(&snapshot).unwrap();

        let (delta, _) = server
            .add_entry(
                None,
                &TocEntry::Text {
                    title: "Synced".to_string(),
                },
            )
            .unwrap();

        client.apply_updates(&[delta]).unwrap();

        let tree = client.read_tree();
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].entry.title(), "Synced");
    }
}
