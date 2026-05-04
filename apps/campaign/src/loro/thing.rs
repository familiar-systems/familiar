//! Typed Loro wrapper for Thing documents (ProseMirror-compatible).
//!
//! A Thing page is a rich-text document synced between the TipTap editor (via
//! `loro-prosemirror`) and the Rust backend. The document follows the
//! ProseMirror Loro convention: each node is a `LoroMap` with `nodeName`,
//! `attributes`, and `children` keys.
//!
//! ProseMirror schema constants (key/container names) live in
//! `familiar_systems_campaign_shared::loro::prosemirror`.

use loro::LoroDoc;

use crate::domain::crdt::{CrdtDoc, DocError, Snapshot, VersionVector};

/// Typed wrapper around a `LoroDoc` for Thing pages.
///
/// Unlike `LoroTocDoc`, this document's content is defined by the ProseMirror
/// schema (TipTap node specs in `packages/editor/`). The `loro-prosemirror`
/// bridge handles sync on the TypeScript side. This wrapper provides the Rust
/// side's read/write interface using the same container conventions.
pub struct LoroThingDoc {
    doc: LoroDoc,
}

#[allow(clippy::new_without_default)]
impl LoroThingDoc {
    /// Create a new empty Thing document.
    pub fn new() -> Self {
        Self {
            doc: LoroDoc::new(),
        }
    }

    /// Restore from a snapshot blob.
    pub fn from_snapshot(snapshot: &Snapshot) -> Result<Self, DocError> {
        let doc = LoroDoc::new();
        doc.import(snapshot.as_bytes())
            .map_err(|e| DocError::ImportSnapshot(e.to_string()))?;
        Ok(Self { doc })
    }
}

impl CrdtDoc for LoroThingDoc {
    fn version(&self) -> VersionVector {
        VersionVector(self.doc.oplog_vv().encode())
    }

    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), DocError> {
        for (i, update) in updates.iter().enumerate() {
            self.doc.import(update).map_err(|e| {
                DocError::ApplyUpdate(format!(
                    "update {i} ({} bytes): {e}",
                    update.len()
                ))
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

    #[test]
    fn new_doc_exports_non_empty_snapshot() {
        let doc = LoroThingDoc::new();
        let snap = doc.export_snapshot().unwrap();
        assert!(!snap.as_bytes().is_empty());
    }

    #[test]
    fn round_trip_through_trait() {
        let mut doc = LoroThingDoc::new();

        // Simulate a client creating content via a raw LoroDoc
        let client = loro::LoroDoc::new();
        let text = client.get_text("content");
        text.insert(0, "hello thing").unwrap();
        let update = client
            .export(loro::ExportMode::Updates {
                from: Default::default(),
            })
            .unwrap();

        doc.apply_updates(&[update]).unwrap();

        let snapshot = doc.export_snapshot().unwrap();
        let doc2 = LoroThingDoc::from_snapshot(&snapshot).unwrap();

        let val = doc2.debug_value().unwrap();
        let json = serde_json::to_string(&val).unwrap();
        assert!(json.contains("hello thing"), "Got: {json}");
    }

    #[test]
    fn version_advances_after_update() {
        let mut doc = LoroThingDoc::new();
        let v1 = doc.version();

        let client = loro::LoroDoc::new();
        let text = client.get_text("content");
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
