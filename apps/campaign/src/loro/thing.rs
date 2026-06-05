//! Typed Loro wrapper for Thing documents.
//!
//! A Thing has two root containers in its LoroDoc:
//! - `"meta"` (LoroMap): title and status as LWW strings
//! - `"content"` (LoroMap): ProseMirror document root for the main body
//!
//! ## Persistence lifecycle
//!
//! The LoroDoc is transient. At rest, blocks are rows in the `blocks` table.
//! `from_blocks` reconstructs the doc from those rows. `extract_blocks`
//! decomposes it back. CRDT history is intentionally discarded across
//! checkout cycles (no tombstone accumulation).
//!
//! The doc's Loro binary snapshot is used only for the wire protocol
//! (client-join sends `ExportMode::Snapshot`).

use std::borrow::Cow;

use loro::{LoroDoc, LoroMap, LoroValue, ValueOrContainer};

use familiar_systems_campaign_shared::loro::thing::{
    CONTAINER_CONTENT, CONTAINER_META, KEY_STATUS, KEY_TITLE,
};
use familiar_systems_campaign_shared::status::Status;

use crate::domain::crdt::doc::{CrdtDoc, DocError, Snapshot, VersionVector};
use crate::loro::block_codec;

pub struct LoroThingDoc {
    doc: LoroDoc,
}

#[allow(clippy::new_without_default)]
impl LoroThingDoc {
    /// Create a new empty Thing document with initialized containers.
    pub fn new() -> Self {
        let doc = LoroDoc::new();
        let meta = doc.get_map(CONTAINER_META);
        meta.insert(KEY_TITLE, "").unwrap();
        meta.insert(KEY_STATUS, Status::GmOnly.as_loro_str())
            .unwrap();
        let _content = doc.get_map(CONTAINER_CONTENT);
        Self { doc }
    }

    /// Reconstruct a Thing document from relational data.
    ///
    /// `block_blobs` must be pre-sorted by ordering. Each blob is the
    /// opaque JSON produced by `block_codec::serialize_block`.
    ///
    /// Returns the doc alongside any [`SkippedBlock`](block_codec::SkippedBlock)s
    /// that could not be reconstructed. Restore is best-effort per block, so a
    /// single corrupt blob is dropped (and reported for the caller to log)
    /// rather than panicking and leaving the Thing un-openable.
    pub fn from_blocks(
        name: &str,
        status: &Status,
        block_blobs: &[Vec<u8>],
    ) -> (Self, Vec<block_codec::SkippedBlock>) {
        let this = Self::new();
        let meta = this.meta();
        meta.insert(KEY_TITLE, name).unwrap();
        meta.insert(KEY_STATUS, status.as_loro_str()).unwrap();

        let content = this.content();
        let skipped = block_codec::restore_content(&content, block_blobs);

        (this, skipped)
    }

    /// Restore from a Loro binary snapshot (used only for CrdtDoc trait,
    /// not for relational persistence).
    pub fn from_snapshot(snapshot: &Snapshot) -> Result<Self, DocError> {
        let doc = LoroDoc::new();
        doc.import(snapshot.as_bytes())
            .map_err(|e| DocError::ImportSnapshot(e.to_string()))?;
        Ok(Self { doc })
    }

    // -- Private helpers --

    fn meta(&self) -> LoroMap {
        self.doc.get_map(CONTAINER_META)
    }

    fn content(&self) -> LoroMap {
        self.doc.get_map(CONTAINER_CONTENT)
    }

    /// Capture version vector, run mutation, export the delta for broadcasting.
    #[allow(dead_code)]
    fn with_delta(&self, f: impl FnOnce()) -> Result<Vec<u8>, String> {
        let vv_before = self.doc.oplog_vv();
        f();
        self.doc
            .export(loro::ExportMode::Updates {
                from: Cow::Owned(vv_before),
            })
            .map_err(|e| format!("failed to export thing update: {e}"))
    }

    // -- Public domain methods --

    /// Read the title from the meta section.
    pub fn read_title(&self) -> Option<String> {
        match self.meta().get(KEY_TITLE)? {
            ValueOrContainer::Value(LoroValue::String(s)) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    }

    /// Read the status from the meta section.
    pub fn read_status(&self) -> Option<Status> {
        match self.meta().get(KEY_STATUS)? {
            ValueOrContainer::Value(LoroValue::String(s)) => Status::from_loro_str(&s),
            _ => None,
        }
    }

    /// Extract blocks from the content section for relational persistence.
    pub fn extract_blocks(&self) -> Vec<block_codec::ExtractedBlock> {
        block_codec::extract_blocks(&self.content())
    }
}

impl CrdtDoc for LoroThingDoc {
    fn version(&self) -> VersionVector {
        VersionVector(self.doc.oplog_vv().encode())
    }

    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), DocError> {
        for (i, update) in updates.iter().enumerate() {
            self.doc.import(update).map_err(|e| {
                DocError::ApplyUpdate(format!("thing update {i} ({} bytes): {e}", update.len()))
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
    use familiar_systems_campaign_shared::loro::prosemirror::{
        ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
    };
    use loro::{LoroList, LoroText};

    #[test]
    fn new_doc_has_meta_and_content() {
        let doc = LoroThingDoc::new();
        let val = doc.debug_value().unwrap();
        assert!(val.get(CONTAINER_META).is_some());
        assert!(val.get(CONTAINER_CONTENT).is_some());
    }

    #[test]
    fn from_blocks_populates_meta() {
        let (doc, _) = LoroThingDoc::from_blocks("Korgath", &Status::Known, &[]);
        assert_eq!(doc.read_title(), Some("Korgath".to_string()));
        assert_eq!(doc.read_status(), Some(Status::Known));
    }

    #[test]
    fn from_blocks_populates_content() {
        let heading_blob = serde_json::to_vec(&serde_json::json!({
            NODE_NAME_KEY: "heading",
            ATTRIBUTES_KEY: { "level": 1 },
            CHILDREN_KEY: ["The Iron Citadel"]
        }))
        .unwrap();
        let para_blob = serde_json::to_vec(&serde_json::json!({
            NODE_NAME_KEY: "paragraph",
            ATTRIBUTES_KEY: {},
            CHILDREN_KEY: ["A fortress at the edge of the world."]
        }))
        .unwrap();

        let (doc, _) =
            LoroThingDoc::from_blocks("Iron Citadel", &Status::GmOnly, &[heading_blob, para_blob]);

        let deep: serde_json::Value = doc.debug_value().unwrap();
        let children = &deep[CONTAINER_CONTENT][CHILDREN_KEY];
        assert_eq!(children.as_array().unwrap().len(), 2);
        assert_eq!(children[0][NODE_NAME_KEY], "heading");
        assert_eq!(children[1][NODE_NAME_KEY], "paragraph");
    }

    #[test]
    fn from_blocks_empty() {
        let (doc, _) = LoroThingDoc::from_blocks("Empty Thing", &Status::GmOnly, &[]);
        let blocks = doc.extract_blocks();
        assert!(blocks.is_empty());
    }

    #[test]
    fn extract_blocks_round_trip() {
        let heading_blob = serde_json::to_vec(&serde_json::json!({
            NODE_NAME_KEY: "heading",
            ATTRIBUTES_KEY: { "level": 2 },
            CHILDREN_KEY: ["Chapter One"]
        }))
        .unwrap();

        let (doc, _) =
            LoroThingDoc::from_blocks("Test", &Status::Known, std::slice::from_ref(&heading_blob));
        let extracted = doc.extract_blocks();
        assert_eq!(extracted.len(), 1);

        let (doc2, _) =
            LoroThingDoc::from_blocks("Test", &Status::Known, &[extracted[0].content.clone()]);

        assert_eq!(doc.debug_value(), doc2.debug_value());
    }

    #[test]
    fn read_status_all_variants() {
        for status in [Status::GmOnly, Status::Known, Status::Retconned] {
            let (doc, _) = LoroThingDoc::from_blocks("Test", &status, &[]);
            assert_eq!(doc.read_status(), Some(status));
        }
    }

    #[test]
    fn snapshot_round_trip() {
        let heading_blob = serde_json::to_vec(&serde_json::json!({
            NODE_NAME_KEY: "heading",
            ATTRIBUTES_KEY: { "level": 1 },
            CHILDREN_KEY: ["Snapshot Test"]
        }))
        .unwrap();

        let (doc, _) = LoroThingDoc::from_blocks("Snap", &Status::Known, &[heading_blob]);
        let snapshot = doc.export_snapshot().unwrap();
        let doc2 = LoroThingDoc::from_snapshot(&snapshot).unwrap();

        assert_eq!(doc.debug_value(), doc2.debug_value());
        assert_eq!(doc2.read_title(), Some("Snap".to_string()));
    }

    #[test]
    fn convergence_after_client_updates() {
        let (doc, _) = LoroThingDoc::from_blocks("Server", &Status::GmOnly, &[]);
        let snapshot = doc.export_snapshot().unwrap();

        // Simulate a client that received the snapshot and adds content
        let client = LoroDoc::new();
        client.import(snapshot.as_bytes()).unwrap();
        let content = client.get_map(CONTAINER_CONTENT);
        content.insert(NODE_NAME_KEY, "doc").unwrap();
        content
            .insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        let children = content
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();
        let para = children.insert_container(0, LoroMap::new()).unwrap();
        para.insert(NODE_NAME_KEY, "paragraph").unwrap();
        para.insert_container(ATTRIBUTES_KEY, LoroMap::new())
            .unwrap();
        let p_children = para
            .insert_container(CHILDREN_KEY, LoroList::new())
            .unwrap();
        let text = p_children.insert_container(0, LoroText::new()).unwrap();
        text.insert(0, "Client wrote this.").unwrap();
        client.commit();

        let update = client
            .export(loro::ExportMode::Updates {
                from: Cow::Owned(doc.doc.oplog_vv()),
            })
            .unwrap();

        let mut server = doc;
        server.apply_updates(&[update]).unwrap();

        assert_eq!(server.debug_value(), Some(client.get_deep_value().into()));
    }

    #[test]
    fn version_advances_after_update() {
        let mut doc = LoroThingDoc::new();
        let v1 = doc.version();

        let client = LoroDoc::new();
        let text = client.get_text("scratch");
        text.insert(0, "abc").unwrap();
        let update = client
            .export(loro::ExportMode::Updates {
                from: Default::default(),
            })
            .unwrap();

        doc.apply_updates(&[update]).unwrap();
        let v2 = doc.version();
        assert_ne!(v1.as_bytes(), v2.as_bytes());
    }

    #[test]
    fn apply_bad_update_returns_error() {
        let mut doc = LoroThingDoc::new();
        let result = doc.apply_updates(&[vec![0xFF, 0xFE, 0xFD]]);
        assert!(result.is_err());
    }
}
