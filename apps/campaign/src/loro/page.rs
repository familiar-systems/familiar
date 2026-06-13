//! Typed Loro wrapper for Page documents.
//!
//! A Page has a `"meta"` root container plus one root container per section.
//! For Entity/Template pages: `"meta"` (LoroMap: title, status, kind as LWW
//! strings), `"preamble"` (the bounded index card), and `"body"` (the freeform
//! ProseMirror document root). The ordered section list comes from
//! [`PageKind::sections`].
//!
//! ## Persistence lifecycle
//!
//! The LoroDoc is transient. At rest, blocks are rows in the `blocks` table,
//! each tagged with its `section`. `from_blocks` reconstructs the doc from those
//! rows (per section); `extract_sections` decomposes it back. CRDT history is
//! intentionally discarded across checkout cycles (no tombstone accumulation).
//!
//! The doc's Loro binary snapshot is used only for the wire protocol
//! (client-join sends `ExportMode::Snapshot`).

use std::borrow::Cow;

use loro::{LoroDoc, LoroMap, LoroValue, ValueOrContainer};

use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::loro::page::{
    CONTAINER_META, KEY_KIND, KEY_STATUS, KEY_TITLE,
};
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;

use crate::domain::crdt::doc::{CrdtDoc, DocError, Snapshot, VersionVector};
use crate::loro::block_codec;

pub struct LoroPageDoc {
    doc: LoroDoc,
    /// This page's kind: the immutable authority for its section layout. Stamped
    /// from the `pages` row at construction and never mutated by a CRDT op, so it
    /// cannot drift or be corrupted over the wire. Distinct from `meta.kind`,
    /// which is also written but only as the client's render projection: the
    /// destructive flush ([`extract_sections`](Self::extract_sections)) must read
    /// this incorruptible field, never the client-writable `meta.kind`. A page
    /// never changes kind.
    kind: PageKind,
}

impl LoroPageDoc {
    /// Build a Page document from its identity and per-section block blobs.
    ///
    /// This is the **only** constructor: genesis passes `build_new_page`'s seed
    /// blocks, restore passes the `blocks` rows grouped by section. `sections` is a
    /// list of `(section_name, blobs)`; each section's blobs must be pre-sorted by
    /// ordering, and each blob is the opaque JSON produced by
    /// `block_codec::serialize_block`.
    ///
    /// Construction is driven off the kind's declared
    /// [`sections()`](PageKind::sections): every declared section container is
    /// initialized to a valid (possibly empty) ProseMirror doc, a caller that omits
    /// a section yields an empty one, and rows under an undeclared section are
    /// ignored. `meta` carries title/status/kind; `meta.kind` is the client's
    /// wire/render projection (the SPA reads it to pick the page experience), while
    /// the persistence authority is the [`kind`](Self::kind) field.
    ///
    /// Returns the doc alongside any [`SkippedBlock`](block_codec::SkippedBlock)s
    /// that could not be reconstructed. Restore is best-effort per block, so a
    /// single corrupt blob is dropped (and reported for the caller to log) rather
    /// than panicking and leaving the Page un-openable.
    pub fn from_blocks(
        name: &str,
        status: &Status,
        kind: &PageKind,
        sections: &[(&str, Vec<Vec<u8>>)],
    ) -> (Self, Vec<block_codec::SkippedBlock>) {
        let doc = LoroDoc::new();
        let meta = doc.get_map(CONTAINER_META);
        meta.insert(KEY_TITLE, name).unwrap();
        meta.insert(KEY_STATUS, status.as_loro_str()).unwrap();
        meta.insert(KEY_KIND, kind.as_loro_str()).unwrap();
        let this = Self { doc, kind: *kind };

        let mut skipped = Vec::new();
        for section_name in kind.sections() {
            let blobs = sections
                .iter()
                .find(|(name, _)| name == section_name)
                .map(|(_, blobs)| blobs.as_slice())
                .unwrap_or(&[]);
            let container = this.section(section_name);
            skipped.extend(block_codec::restore_content(&container, blobs));
        }

        (this, skipped)
    }

    // -- Private helpers --

    fn meta(&self) -> LoroMap {
        self.doc.get_map(CONTAINER_META)
    }

    /// The root LoroMap for a named section (e.g. `preamble`, `body`). The name
    /// is a section/container constant; root containers are addressed by name.
    fn section(&self, name: &str) -> LoroMap {
        self.doc.get_map(name)
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
            .map_err(|e| format!("failed to export page update: {e}"))
    }

    // -- Public domain methods --

    /// Read the title from the meta section.
    pub fn read_title(&self) -> Option<String> {
        match self.meta().get(KEY_TITLE)? {
            ValueOrContainer::Value(LoroValue::String(s)) if !s.is_empty() => Some(s.to_string()),
            _ => None,
        }
    }

    /// Title for display, or a loud, deterministic recovery marker when
    /// `meta.title` is unexpectedly empty. Pages are created name-first, so an
    /// empty title should never happen; if it does, we surface it visibly and
    /// editably (the GM can just rename it) rather than panicking or silently
    /// dropping the node update. The `{id}` makes the broken page findable in the
    /// ToC. Distinct from [`read_title`](Self::read_title) -> `Option`, which
    /// `name_sync` uses to tell a real title from "leave `pages.name` alone".
    pub fn read_title_or_recovery_marker(&self, id: &PageId) -> String {
        self.read_title()
            .unwrap_or_else(|| format!("ERROR LOADING TITLE {}", id.0))
    }

    /// Read the status from the meta section.
    pub fn read_status(&self) -> Option<Status> {
        match self.meta().get(KEY_STATUS)? {
            ValueOrContainer::Value(LoroValue::String(s)) => Status::from_loro_str(&s),
            _ => None,
        }
    }

    /// This page's kind, the immutable identity field (not a re-read of the
    /// client-writable `meta.kind`). Infallible: every constructor stamps it from
    /// the `pages` row. This is the read-path accessor for server logic and the
    /// future serialization compiler.
    pub fn kind(&self) -> PageKind {
        self.kind
    }

    /// Extract each section's blocks for relational persistence, tagged with the
    /// section (container) name. Ordering is per-section: each container's
    /// children index independently from 0, so blocks sort within their section,
    /// not globally. Driven off the immutable [`kind`](Self::kind) field (the
    /// authoritative row value, the same set `from_blocks` restored from), so this
    /// destructive prune-then-upsert never trusts the client-writable `meta.kind`.
    pub fn extract_sections(&self) -> Vec<(&'static str, Vec<block_codec::ExtractedBlock>)> {
        self.kind
            .sections()
            .iter()
            .map(|&name| (name, block_codec::extract_blocks(&self.section(name))))
            .collect()
    }
}

impl CrdtDoc for LoroPageDoc {
    fn version(&self) -> VersionVector {
        VersionVector(self.doc.oplog_vv().encode())
    }

    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), DocError> {
        for (i, update) in updates.iter().enumerate() {
            self.doc.import(update).map_err(|e| {
                DocError::ApplyUpdate(format!("page update {i} ({} bytes): {e}", update.len()))
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

    fn debug_value(&self) -> Option<serde_json::Value> {
        Some(self.doc.get_deep_value().into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use familiar_systems_campaign_shared::loro::page::{CONTAINER_BODY, CONTAINER_PREAMBLE};
    use familiar_systems_campaign_shared::loro::prosemirror::{
        ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
    };
    use loro::{LoroList, LoroText};

    #[test]
    fn from_blocks_creates_meta_and_sections() {
        let (doc, _) = LoroPageDoc::from_blocks("Test", &Status::GmOnly, &PageKind::Entity, &[]);
        let val = doc.debug_value().unwrap();
        assert!(val.get(CONTAINER_META).is_some());
        assert!(val.get(CONTAINER_PREAMBLE).is_some());
        assert!(val.get(CONTAINER_BODY).is_some());
    }

    #[test]
    fn from_blocks_populates_meta() {
        let (doc, _) = LoroPageDoc::from_blocks("Korgath", &Status::Known, &PageKind::Entity, &[]);
        assert_eq!(doc.read_title(), Some("Korgath".to_string()));
        assert_eq!(doc.read_status(), Some(Status::Known));
    }

    #[test]
    fn read_title_or_recovery_marker_falls_back_on_empty() {
        let id = PageId::generate();

        // An empty name is the should-never-happen state (pages are created
        // name-first); the marker is deterministic and carries the id so the
        // page is findable in the ToC.
        let (empty, _) = LoroPageDoc::from_blocks("", &Status::GmOnly, &PageKind::Entity, &[]);
        assert_eq!(empty.read_title(), None);
        assert_eq!(
            empty.read_title_or_recovery_marker(&id),
            format!("ERROR LOADING TITLE {}", id.0),
        );

        // A real title passes straight through, untouched.
        let (named, _) =
            LoroPageDoc::from_blocks("Korgath", &Status::GmOnly, &PageKind::Entity, &[]);
        assert_eq!(named.read_title_or_recovery_marker(&id), "Korgath");
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

        let (doc, _) = LoroPageDoc::from_blocks(
            "Iron Citadel",
            &Status::GmOnly,
            &PageKind::Entity,
            &[(CONTAINER_BODY, vec![heading_blob, para_blob])],
        );

        let deep: serde_json::Value = doc.debug_value().unwrap();
        let children = &deep[CONTAINER_BODY][CHILDREN_KEY];
        assert_eq!(children.as_array().unwrap().len(), 2);
        assert_eq!(children[0][NODE_NAME_KEY], "heading");
        assert_eq!(children[1][NODE_NAME_KEY], "paragraph");
    }

    #[test]
    fn from_blocks_empty() {
        let (doc, _) =
            LoroPageDoc::from_blocks("Empty Page", &Status::GmOnly, &PageKind::Entity, &[]);
        let sections = doc.extract_sections();
        assert!(sections.iter().all(|(_, blocks)| blocks.is_empty()));
    }

    #[test]
    fn extract_sections_round_trip() {
        let heading_blob = serde_json::to_vec(&serde_json::json!({
            NODE_NAME_KEY: "heading",
            ATTRIBUTES_KEY: { "level": 2 },
            CHILDREN_KEY: ["Chapter One"]
        }))
        .unwrap();

        let (doc, _) = LoroPageDoc::from_blocks(
            "Test",
            &Status::Known,
            &PageKind::Entity,
            &[(CONTAINER_BODY, vec![heading_blob])],
        );
        let sections = doc.extract_sections();
        let body = sections.iter().find(|(n, _)| *n == CONTAINER_BODY).unwrap();
        assert_eq!(body.1.len(), 1);
        // The other section (preamble) extracts cleanly as empty.
        let preamble = sections
            .iter()
            .find(|(n, _)| *n == CONTAINER_PREAMBLE)
            .unwrap();
        assert!(preamble.1.is_empty());

        let (doc2, _) = LoroPageDoc::from_blocks(
            "Test",
            &Status::Known,
            &PageKind::Entity,
            &[(CONTAINER_BODY, vec![body.1[0].content.clone()])],
        );

        assert_eq!(doc.debug_value(), doc2.debug_value());
    }

    #[test]
    fn sections_have_independent_ordering() {
        // Two blocks in body, one in preamble. Each section's ordering starts at
        // 0 independently: blocks sort within their section, not globally.
        let blob = |text: &str| {
            serde_json::to_vec(&serde_json::json!({
                NODE_NAME_KEY: "paragraph",
                ATTRIBUTES_KEY: {},
                CHILDREN_KEY: [text],
            }))
            .unwrap()
        };

        let (doc, _) = LoroPageDoc::from_blocks(
            "Sectioned",
            &Status::GmOnly,
            &PageKind::Entity,
            &[
                (CONTAINER_PREAMBLE, vec![blob("card")]),
                (CONTAINER_BODY, vec![blob("first"), blob("second")]),
            ],
        );

        let sections = doc.extract_sections();
        let by = |name: &str| {
            sections
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, blocks)| blocks)
                .unwrap()
        };
        let preamble = by(CONTAINER_PREAMBLE);
        let body = by(CONTAINER_BODY);
        assert_eq!(preamble.iter().map(|b| b.ordering).collect::<Vec<_>>(), [0]);
        assert_eq!(
            body.iter().map(|b| b.ordering).collect::<Vec<_>>(),
            [0, 1],
            "body orderings restart at 0, independent of the preamble"
        );
    }

    #[test]
    fn read_status_all_variants() {
        for status in [Status::GmOnly, Status::Known, Status::Retconned] {
            let (doc, _) = LoroPageDoc::from_blocks("Test", &status, &PageKind::Entity, &[]);
            assert_eq!(doc.read_status(), Some(status));
        }
    }

    #[test]
    fn kind_round_trips() {
        for kind in [PageKind::Entity, PageKind::Template] {
            let (doc, _) = LoroPageDoc::from_blocks("Test", &Status::GmOnly, &kind, &[]);
            assert_eq!(doc.kind(), kind);
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

        let (doc, _) = LoroPageDoc::from_blocks(
            "Snap",
            &Status::Known,
            &PageKind::Entity,
            &[(CONTAINER_BODY, vec![heading_blob])],
        );
        let snapshot = doc.export_snapshot().unwrap();

        // A joining client imports the snapshot into its own raw LoroDoc (the
        // real wire path: the server exports, the client's Loro doc imports).
        // The reconstructed client state must match the server's full doc.
        let client = LoroDoc::new();
        client.import(snapshot.as_bytes()).unwrap();
        assert_eq!(doc.debug_value(), Some(client.get_deep_value().into()));
    }

    #[test]
    fn convergence_after_client_updates() {
        let (doc, _) = LoroPageDoc::from_blocks("Server", &Status::GmOnly, &PageKind::Entity, &[]);
        let snapshot = doc.export_snapshot().unwrap();

        // Simulate a client that received the snapshot and adds content
        let client = LoroDoc::new();
        client.import(snapshot.as_bytes()).unwrap();
        let content = client.get_map(CONTAINER_BODY);
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
        let (mut doc, _) =
            LoroPageDoc::from_blocks("Test", &Status::GmOnly, &PageKind::Entity, &[]);
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
        let (mut doc, _) =
            LoroPageDoc::from_blocks("Test", &Status::GmOnly, &PageKind::Entity, &[]);
        let result = doc.apply_updates(&[vec![0xFF, 0xFE, 0xFD]]);
        assert!(result.is_err());
    }
}
