//! Typed Loro wrapper for Thing documents (ProseMirror-compatible).
//!
//! A Thing page is a rich-text document synced between the TipTap editor (via
//! `loro-prosemirror`) and the Rust backend. The document follows the ProseMirror
//! Loro convention: each node is a `LoroMap` with `nodeName`, `attributes`, and
//! `children` keys.
//!
//! The Rust side reads and writes these documents for:
//! - AI serialization compiler (LoroDoc -> agent markdown)
//! - Mention rename propagation (walk tree, update mention labels)
//! - Suggestion mark application (target block IDs, add annotation metadata)
//!
//! Key constants are in `super::prosemirror`.

use loro::LoroDoc;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::{CrdtDoc, Snapshot, VersionVector};
use crate::id::ThingId;

/// Lightweight reference to a Thing: its ID and display name.
///
/// Used by CampaignVocabulary, ToC, RelationshipGraph, and anywhere
/// that needs to reference a Thing without loading its full document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct ThingHandle {
    pub id: ThingId,
    pub name: String,
}

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
    pub fn from_snapshot(snapshot: &Snapshot) -> Result<Self, String> {
        let doc = LoroDoc::new();
        doc.import(snapshot.as_bytes())
            .map_err(|e| format!("failed to import thing snapshot: {e}"))?;
        Ok(Self { doc })
    }
}

impl CrdtDoc for LoroThingDoc {
    fn get_version(&self) -> VersionVector {
        VersionVector(self.doc.oplog_vv().encode())
    }

    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), String> {
        for (i, update) in updates.iter().enumerate() {
            self.doc.import(update).map_err(|e| {
                format!(
                    "failed to import thing update {i} ({} bytes): {e}",
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
            .map_err(|e| format!("failed to export thing snapshot: {e}"))
    }

    fn import_snapshot(&mut self, data: &Snapshot) -> Result<(), String> {
        self.doc
            .import(data.as_bytes())
            .map(|_| ())
            .map_err(|e| format!("failed to import thing snapshot: {e}"))
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
        let v1 = doc.get_version();

        let client = loro::LoroDoc::new();
        let text = client.get_text("content");
        text.insert(0, "abc").unwrap();
        let update = client
            .export(loro::ExportMode::Updates {
                from: Default::default(),
            })
            .unwrap();

        doc.apply_updates(&[update]).unwrap();
        let v2 = doc.get_version();
        assert_ne!(v1.as_bytes(), v2.as_bytes());
    }

    #[test]
    fn apply_bad_update_returns_error() {
        let mut doc = LoroThingDoc::new();
        let result = doc.apply_updates(&[vec![0xFF, 0xFE, 0xFD]]);
        assert!(result.is_err());
    }
}
