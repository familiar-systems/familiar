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
//! rows (bucketed by section, one container per declared section, empty sections
//! seeded with a paragraph); `extract_sections` decomposes it back. CRDT history
//! is intentionally discarded across checkout cycles (no tombstone accumulation).
//!
//! The doc's Loro binary snapshot is used only for the wire protocol
//! (client-join sends `ExportMode::Snapshot`).

use std::borrow::Cow;
use std::collections::HashMap;

use loro::{LoroDoc, LoroMap, LoroValue, ValueOrContainer};

use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::loro::page::{
    CONTAINER_META, KEY_KIND, KEY_STATUS, KEY_TITLE, Section,
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
    /// Build a Page document from its identity and at-rest block rows.
    ///
    /// This is the **only** constructor. `rows` is the flat `(section, blob)`
    /// stream straight from the `blocks` table (restore) or empty (genesis); a
    /// row's `section` is the at-rest name of its Loro container, and each blob
    /// is the opaque JSON produced by `block_codec::serialize_block`. Rows are
    /// bucketed by section here, so callers never have to know a kind's section
    /// layout -- that is the whole point: **section layout is a property of the
    /// kind, owned by this constructor, not the create path.**
    ///
    /// Construction is driven off the kind's declared
    /// [`sections()`](PageKind::sections), in order. Each declared section's rows
    /// are restored into its container; a declared section with **no** rows is
    /// seeded with one empty paragraph (via `mint`) so it opens as a schema-valid,
    /// editable ProseMirror doc rather than an empty, uneditable `doc`. Rows tagged
    /// with a section the kind does not declare bind to no container and are
    /// dropped (logged loudly) -- the one section/kind drift guard.
    ///
    /// `mint` supplies the seed blocks' ids (production passes `BlockId::generate`).
    /// It is injected rather than called internally so seeding is an explicit
    /// effect and tests stay deterministic. `meta` carries title/status/kind;
    /// `meta.kind` is the client's wire/render projection, while the persistence
    /// authority is the [`kind`](Self::kind) field.
    ///
    /// Returns the doc alongside any [`SkippedBlock`](block_codec::SkippedBlock)s
    /// that could not be reconstructed. Restore is best-effort per block, so a
    /// single corrupt blob is dropped (and reported for the caller to log) rather
    /// than panicking and leaving the Page un-openable.
    pub fn from_blocks(
        name: &str,
        status: &Status,
        kind: &PageKind,
        rows: impl IntoIterator<Item = (Section, Vec<u8>)>,
        mut mint: impl FnMut() -> BlockId,
    ) -> (Self, Vec<block_codec::SkippedBlock>) {
        let doc = LoroDoc::new();
        let meta = doc.get_map(CONTAINER_META);
        meta.insert(KEY_TITLE, name).unwrap();
        meta.insert(KEY_STATUS, status.as_loro_str()).unwrap();
        meta.insert(KEY_KIND, kind.as_loro_str()).unwrap();
        let this = Self { doc, kind: *kind };

        // Bucket rows by section. Rows arrive already typed as `Section` -- the
        // `SectionCol` boundary converted them at the sea-orm edge and rejected
        // any unknown at-rest token before we got here -- so this is pure enum
        // bucketing, no string handling. Restore order within a section is the
        // input order, which the restore query (`ORDER BY section, ordering`)
        // already makes correct.
        let mut by_section: HashMap<Section, Vec<Vec<u8>>> = HashMap::new();
        for (section, content) in rows {
            by_section.entry(section).or_default().push(content);
        }

        // Initialize every section the kind declares, in order. Restore its rows;
        // an empty section is seeded with one empty paragraph so it opens as a
        // schema-valid, editable ProseMirror doc (a `doc` with one `block+` child).
        let mut skipped = Vec::new();
        for &section in kind.sections() {
            let container = this.section(section.as_str());
            match by_section.remove(&section) {
                Some(blobs) => skipped.extend(block_codec::restore_content(&container, &blobs)),
                None => {
                    let seed = block_codec::empty_paragraph_blob(&mint());
                    skipped.extend(block_codec::restore_content(&container, &[seed]));
                }
            }
        }

        // Rows left in `by_section` are a *known* section this kind does not
        // declare (a kind-mismatch): they bind to no container, so drop them,
        // logged loudly, never rebound. Unreachable today (Entity and Template
        // share both sections); live once a kind declares a divergent set
        // (Skill/Session). Unknown at-rest tokens never reach here -- `SectionCol`
        // rejects them at the read.
        for (orphan, dropped) in by_section {
            tracing::error!(
                section = orphan.as_str(),
                count = dropped.len(),
                ?kind,
                "blocks tagged with a section this page kind does not declare; dropped",
            );
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
    pub fn extract_sections(&self) -> Vec<(Section, Vec<block_codec::ExtractedBlock>)> {
        self.kind
            .sections()
            .iter()
            .map(|&section| {
                (
                    section,
                    block_codec::extract_blocks(&self.section(section.as_str())),
                )
            })
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
    use familiar_systems_campaign_shared::loro::prosemirror::{
        ATTRIBUTES_KEY, CHILDREN_KEY, NODE_NAME_KEY,
    };
    use loro::{LoroList, LoroText};

    /// No at-rest rows: every declared section gets seeded. Typed so the empty
    /// `IntoIterator` resolves without annotations at each call site.
    fn no_rows() -> Vec<(Section, Vec<u8>)> {
        Vec::new()
    }

    /// Flatten `(section, blobs)` specs into the flat `(section, blob)` row stream
    /// `from_blocks` consumes -- keeps tests reading close to "section -> blocks".
    fn rows(specs: impl IntoIterator<Item = (Section, Vec<Vec<u8>>)>) -> Vec<(Section, Vec<u8>)> {
        specs
            .into_iter()
            .flat_map(|(section, blobs)| blobs.into_iter().map(move |b| (section, b)))
            .collect()
    }

    fn paragraph_blob(text: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            NODE_NAME_KEY: "paragraph",
            ATTRIBUTES_KEY: {},
            CHILDREN_KEY: [text],
        }))
        .unwrap()
    }

    #[test]
    fn from_blocks_creates_meta_and_sections() {
        let (doc, _) = LoroPageDoc::from_blocks(
            "Test",
            &Status::GmOnly,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
        let val = doc.debug_value().unwrap();
        assert!(val.get(CONTAINER_META).is_some());
        assert!(val.get(Section::Preamble.as_str()).is_some());
        assert!(val.get(Section::Body.as_str()).is_some());
    }

    #[test]
    fn from_blocks_populates_meta() {
        let (doc, _) = LoroPageDoc::from_blocks(
            "Korgath",
            &Status::Known,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
        assert_eq!(doc.read_title(), Some("Korgath".to_string()));
        assert_eq!(doc.read_status(), Some(Status::Known));
    }

    #[test]
    fn read_title_or_recovery_marker_falls_back_on_empty() {
        let id = PageId::generate();

        // An empty name is the should-never-happen state (pages are created
        // name-first); the marker is deterministic and carries the id so the
        // page is findable in the ToC.
        let (empty, _) = LoroPageDoc::from_blocks(
            "",
            &Status::GmOnly,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
        assert_eq!(empty.read_title(), None);
        assert_eq!(
            empty.read_title_or_recovery_marker(&id),
            format!("ERROR LOADING TITLE {}", id.0),
        );

        // A real title passes straight through, untouched.
        let (named, _) = LoroPageDoc::from_blocks(
            "Korgath",
            &Status::GmOnly,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
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
        let para_blob = paragraph_blob("A fortress at the edge of the world.");

        let (doc, _) = LoroPageDoc::from_blocks(
            "Iron Citadel",
            &Status::GmOnly,
            &PageKind::Entity,
            rows([(Section::Body, vec![heading_blob, para_blob])]),
            BlockId::generate,
        );

        let deep: serde_json::Value = doc.debug_value().unwrap();
        let children = &deep[Section::Body.as_str()][CHILDREN_KEY];
        assert_eq!(children.as_array().unwrap().len(), 2);
        assert_eq!(children[0][NODE_NAME_KEY], "heading");
        assert_eq!(children[1][NODE_NAME_KEY], "paragraph");
    }

    #[test]
    fn from_blocks_seeds_each_empty_section() {
        // No rows: every declared section is seeded with one empty paragraph so it
        // opens schema-valid and editable, with a stable blockId persisted at
        // genesis (the seed's `attributes.blockId` becomes its row id).
        let (doc, _) = LoroPageDoc::from_blocks(
            "Empty Page",
            &Status::GmOnly,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
        let sections = doc.extract_sections();
        assert_eq!(sections.len(), PageKind::Entity.sections().len());
        for (section, blocks) in &sections {
            assert_eq!(
                blocks.len(),
                1,
                "section {} seeded with one paragraph",
                section.as_str()
            );
            let v: serde_json::Value = serde_json::from_slice(&blocks[0].content).unwrap();
            assert_eq!(v[NODE_NAME_KEY], "paragraph");
            assert!(blocks[0].id.is_some(), "seed carries a stable blockId");
        }
    }

    // The "undeclared section is dropped" path moved: an *unknown* at-rest token
    // can no longer reach `from_blocks` (rows arrive typed as `Section`, the
    // `SectionCol` boundary having rejected unknowns at the read -- see the
    // `section_col_rejects_unknown_token` test in `entities::columns`). The
    // remaining in-`from_blocks` orphan drop is the kind-mismatch case (a known
    // section a kind doesn't declare), which is unreachable until a kind declares
    // a divergent section set (Skill/Session); its test lands with that work.

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
            rows([(Section::Body, vec![heading_blob])]),
            BlockId::generate,
        );
        let sections = doc.extract_sections();
        let body = sections.iter().find(|(n, _)| *n == Section::Body).unwrap();
        assert_eq!(body.1.len(), 1);
        // The preamble had no rows, so it was seeded with one paragraph.
        let preamble = sections
            .iter()
            .find(|(n, _)| *n == Section::Preamble)
            .unwrap();
        assert_eq!(preamble.1.len(), 1);

        // Round-trip the FULL extraction (every section, including the seeded
        // preamble) so rebuilding re-seeds nothing and the doc comes back byte
        // identical -- `mint` is never called the second time.
        let all_rows: Vec<(Section, Vec<u8>)> = sections
            .iter()
            .flat_map(|(section, blocks)| blocks.iter().map(move |b| (*section, b.content.clone())))
            .collect();
        let (doc2, _) = LoroPageDoc::from_blocks(
            "Test",
            &Status::Known,
            &PageKind::Entity,
            all_rows,
            BlockId::generate,
        );

        assert_eq!(doc.debug_value(), doc2.debug_value());
    }

    #[test]
    fn sections_have_independent_ordering() {
        // Two blocks in body, one in preamble: both sections are non-empty, so
        // neither is seeded. Each section's ordering starts at 0 independently.
        let (doc, _) = LoroPageDoc::from_blocks(
            "Sectioned",
            &Status::GmOnly,
            &PageKind::Entity,
            rows([
                (Section::Preamble, vec![paragraph_blob("card")]),
                (
                    Section::Body,
                    vec![paragraph_blob("first"), paragraph_blob("second")],
                ),
            ]),
            BlockId::generate,
        );

        let sections = doc.extract_sections();
        let by = |section: Section| {
            sections
                .iter()
                .find(|(n, _)| *n == section)
                .map(|(_, blocks)| blocks)
                .unwrap()
        };
        let preamble = by(Section::Preamble);
        let body = by(Section::Body);
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
            let (doc, _) = LoroPageDoc::from_blocks(
                "Test",
                &status,
                &PageKind::Entity,
                no_rows(),
                BlockId::generate,
            );
            assert_eq!(doc.read_status(), Some(status));
        }
    }

    #[test]
    fn kind_round_trips() {
        for kind in [PageKind::Entity, PageKind::Template, PageKind::Session] {
            let (doc, _) = LoroPageDoc::from_blocks(
                "Test",
                &Status::GmOnly,
                &kind,
                no_rows(),
                BlockId::generate,
            );
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
            rows([(Section::Body, vec![heading_blob])]),
            BlockId::generate,
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
        let (doc, _) = LoroPageDoc::from_blocks(
            "Server",
            &Status::GmOnly,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
        let snapshot = doc.export_snapshot().unwrap();

        // Simulate a client that received the snapshot and adds content
        let client = LoroDoc::new();
        client.import(snapshot.as_bytes()).unwrap();
        let content = client.get_map(Section::Body.as_str());
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
        let (mut doc, _) = LoroPageDoc::from_blocks(
            "Test",
            &Status::GmOnly,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
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
        let (mut doc, _) = LoroPageDoc::from_blocks(
            "Test",
            &Status::GmOnly,
            &PageKind::Entity,
            no_rows(),
            BlockId::generate,
        );
        let result = doc.apply_updates(&[vec![0xFF, 0xFE, 0xFD]]);
        assert!(result.is_err());
    }
}
