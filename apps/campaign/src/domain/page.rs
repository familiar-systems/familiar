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
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;

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
/// TODO: (templates) when `from_template_id` is supported, the template's blocks
/// are cloned in at the call edge — deep-copy each block's content, mint a fresh
/// `BlockId`, preserve its `section`, reset the per-section `ordering` — and fed
/// to `from_blocks` as the initial rows; this also sets `template_id` for lineage.
pub fn build_new_page(id: PageId, name: String, kind: PageKind, status: Status) -> NewPage {
    NewPage {
        id,
        name,
        status,
        kind,
        // Template lineage is unset until template instantiation lands; it will
        // be threaded here alongside `kind: Template`.
        template_id: None,
        blocks: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
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
            );
            assert_eq!(nt.status, status);
        }
    }

    #[test]
    fn kind_is_carried_through() {
        for kind in [PageKind::Entity, PageKind::Session] {
            let nt = build_new_page(PageId::generate(), "X".to_string(), kind, Status::GmOnly);
            assert_eq!(nt.kind, kind);
        }
    }
}
