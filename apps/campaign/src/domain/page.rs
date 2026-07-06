//! Pure builder for bringing a new Page into existence.
//!
//! This is the "service" in the functional-core / imperative-shell sense: a
//! pure function that composes the domain values to persist, with all effects
//! (id minting, the clock, the database, the live CRDT doc) pushed to the
//! edges. It does NOT write anything. The `PageActor`'s genesis path consumes
//! the [`NewPage`] it returns: the actor persists it (via `DbCreatePage`) and
//! builds its own `LoroPageDoc`.
//!
//! Keeping creation logic here, and routing the write through the owning actor,
//! is deliberate: a `PageActor` is the single-threaded consistency boundary
//! for a Page, so every mutation flows through it. A direct write around the
//! actor would drift its in-memory CRDT doc from the database the moment the
//! Page has live subscribers.

use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::loro::page::Section;
use familiar_systems_campaign_shared::loro::toc::TocPageKind;
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;

use crate::loro::block_codec;

/// The subset of [`PageKind`] that genesis through the plain **document-page**
/// path: `preamble` + `body`, persisted via `DbCreatePage`, with no temporal
/// row. Threading this (rather than a full `PageKind`) through `CreatePage` makes
/// `Session` *unrepresentable* on the document path - a session mints a temporal
/// `sessions` row and has its own `CreateSession` workflow, so it can never reach
/// here. That turns the old runtime "unsupported kind" rejection into a
/// compile-time impossibility. A future document-shaped kind (Skill / Memory)
/// joins as a variant, and the compiler points at every site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentPageKind {
    Entity,
    Template,
}

impl DocumentPageKind {
    /// The matching ToC node kind. Total (unlike `TocPageKind::Session`, which
    /// needs a genesis-assigned ordinal), so it replaces the supervisor's
    /// hand-written `kind -> TocPageKind` match rather than restating it.
    pub fn toc_page_kind(self) -> TocPageKind {
        match self {
            DocumentPageKind::Entity => TocPageKind::Entity,
            DocumentPageKind::Template => TocPageKind::Template,
        }
    }
}

impl From<DocumentPageKind> for PageKind {
    fn from(kind: DocumentPageKind) -> Self {
        match kind {
            DocumentPageKind::Entity => PageKind::Entity,
            DocumentPageKind::Template => PageKind::Template,
        }
    }
}

/// A block to persist as part of a new Page. A neutral domain value with no
/// sea-orm dependency; the `DatabaseWriteActor` maps it to a `blocks::ActiveModel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBlock {
    pub id: BlockId,
    /// The section this block belongs to (its Loro container). `ordering` is
    /// relative to this section. Written through `SectionCol` at the DB edge.
    pub section: Section,
    pub ordering: i64,
    pub content: Vec<u8>,
    pub status: Status,
}

/// The complete description of a Page to bring into existence. Produced by
/// [`build_new_page`] and handed to the `PageActor` genesis path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewPage {
    pub id: PageId,
    pub name: String,
    pub status: Status,
    /// What kind of page this is, chosen by the caller. `Entity` and `Template`
    /// are document pages persisted via `DbCreatePage`; `Session` is created via
    /// the supervisor's `CreateSession` workflow (which also mints the temporal
    /// row). All three flow through this builder; only the `kind` differs.
    pub kind: PageKind,
    /// Lineage back to the template this was cloned from, if any. `None` until
    /// template *instantiation* (cloning a template into an entity) lands -
    /// creating a template page itself sets `kind: Template` with no lineage.
    pub template_id: Option<PageId>,
    pub blocks: Vec<NewBlock>,
}

/// Build the description of a new Page.
///
/// Pure: no I/O, no clock, no RNG, deterministic and unit-testable. This is the
/// kernel the future AI `create_page` suggestion path will reuse.
///
/// `blocks` starts empty: a new Page has no authored content. Its sections — and
/// the empty paragraph each is seeded with so it opens schema-valid and editable
/// — are a property of the `kind`, materialized by `LoroPageDoc::from_blocks` at
/// the genesis call edge; the actor then persists exactly what that seeded doc
/// contains. Keeping section layout out of this builder is deliberate: the create
/// path never enumerates sections.
///
/// `template_id` records lineage: `Some` when this entity was cloned from a
/// template (its blocks come in as `seed` at the genesis edge via
/// [`clone_template_blocks`]), `None` for a blank page or a template page itself.
pub fn build_new_page(
    id: PageId,
    name: String,
    kind: PageKind,
    status: Status,
    template_id: Option<PageId>,
) -> NewPage {
    NewPage {
        id,
        name,
        status,
        kind,
        template_id,
        blocks: Vec::new(),
    }
}

/// Clone a template's blocks into the seed rows for a new entity.
///
/// Pure: `mint` is the only effect, injected so tests stay deterministic. Each
/// source block (arriving grouped by section, in `ordering` order) becomes a
/// [`NewBlock`] with a **fresh** id — rewritten *inside* the content blob via
/// [`block_codec::reid_block`], not just on the row — so the clone never shares
/// the template's blockIds and genesis maps its per-block visibility correctly
/// (see [`block_codec::reid_block`]). `section` and per-block `status` carry over;
/// `ordering` resets to 0 at each section boundary.
pub fn clone_template_blocks(
    rows: impl IntoIterator<Item = (Section, Vec<u8>, Status)>,
    mut mint: impl FnMut() -> BlockId,
) -> Vec<NewBlock> {
    let mut cloned = Vec::new();
    let mut section_run: Option<Section> = None;
    let mut ordering = 0i64;
    for (section, content, status) in rows {
        if section_run != Some(section) {
            section_run = Some(section);
            ordering = 0;
        }
        let id = mint();
        cloned.push(NewBlock {
            content: block_codec::reid_block(&content, &id),
            id,
            section,
            ordering,
            status,
        });
        ordering += 1;
    }
    cloned
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn new_page_starts_empty_as_an_entity() {
        // A new Page carries no authored content; its sections (and the empty
        // paragraph each is seeded with) are materialized by `from_blocks` from
        // the kind, not by this builder.
        let id = PageId::generate();
        let new_page = build_new_page(
            id.clone(),
            "Korgath".to_string(),
            PageKind::Entity,
            Status::GmOnly,
            None,
        );

        assert_eq!(new_page.id, id);
        assert_eq!(new_page.name, "Korgath");
        assert_eq!(new_page.status, Status::GmOnly);
        assert_eq!(new_page.kind, PageKind::Entity);
        assert_eq!(new_page.template_id, None);
        assert!(new_page.blocks.is_empty());
    }

    #[test]
    fn status_is_carried_through() {
        for status in [Status::GmOnly, Status::Known, Status::Retconned] {
            let nt = build_new_page(
                PageId::generate(),
                "X".to_string(),
                PageKind::Entity,
                status,
                None,
            );
            assert_eq!(nt.status, status);
        }
    }

    #[test]
    fn kind_is_carried_through() {
        for kind in [PageKind::Entity, PageKind::Session] {
            let nt = build_new_page(
                PageId::generate(),
                "X".to_string(),
                kind,
                Status::GmOnly,
                None,
            );
            assert_eq!(nt.kind, kind);
        }
    }

    #[test]
    fn template_lineage_is_carried_through() {
        let template = PageId::generate();
        let nt = build_new_page(
            PageId::generate(),
            "Grimhollow".to_string(),
            PageKind::Entity,
            Status::GmOnly,
            Some(template.clone()),
        );
        assert_eq!(nt.template_id, Some(template));
    }

    #[test]
    fn clone_template_blocks_mints_fresh_ids_and_preserves_section_status() {
        // Source rows as they arrive from the template's blocks query: grouped by
        // section, in ordering order. Same blob reused so the id-swap is visible.
        let src_id = BlockId::generate();
        let blob = block_codec::empty_paragraph_blob(&src_id);
        let rows = vec![
            (Section::Preamble, blob.clone(), Status::Known),
            (Section::Body, blob.clone(), Status::GmOnly),
            (Section::Body, blob.clone(), Status::Known),
        ];

        let cloned = clone_template_blocks(rows, BlockId::generate);

        // Section + per-block status carry over; ordering resets per section.
        assert_eq!(
            cloned.iter().map(|b| b.section).collect::<Vec<_>>(),
            vec![Section::Preamble, Section::Body, Section::Body],
        );
        assert_eq!(
            cloned.iter().map(|b| b.status).collect::<Vec<_>>(),
            vec![Status::Known, Status::GmOnly, Status::Known],
        );
        assert_eq!(
            cloned.iter().map(|b| b.ordering).collect::<Vec<_>>(),
            vec![0, 0, 1],
        );

        // Every clone has a fresh, unique id, none equal to the source's, and each
        // content blob was actually re-id'd (so it differs from the shared source).
        let ids: HashSet<&BlockId> = cloned.iter().map(|b| &b.id).collect();
        assert_eq!(ids.len(), 3, "clone ids must be unique");
        assert!(cloned.iter().all(|b| b.id != src_id));
        assert!(cloned.iter().all(|b| b.content != blob));
    }
}
