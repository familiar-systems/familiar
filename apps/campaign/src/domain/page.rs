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
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;

/// A block to persist as part of a new Page. A neutral domain value with no
/// sea-orm dependency; the `DatabaseWriteActor` maps it to a `blocks::ActiveModel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBlock {
    pub id: BlockId,
    /// The section this block belongs to (a Loro container / `blocks.section`
    /// value, e.g. `preamble` or `body`). `ordering` is relative to this section.
    pub section: &'static str,
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
    /// What kind of page this is. `build_new_page` always produces `Entity`
    /// today; the `template` kind has no creation path until template
    /// instantiation lands (which will set it here alongside `template_id`).
    pub kind: PageKind,
    /// Lineage back to the template this was cloned from, if any. `None` until
    /// template instantiation lands.
    pub template_id: Option<PageId>,
    pub blocks: Vec<NewBlock>,
}

/// Build the description of a new Page.
///
/// Pure: no I/O, no clock, no RNG. The id, status, and `seed_blocks` are inputs
/// so the function is deterministic and unit-testable. This is the kernel the
/// future AI `create_page` suggestion path will reuse.
///
/// `seed_blocks` is the Page's initial content, tagged per section. Genesis
/// callers seed one empty paragraph per section (so each section opens as a
/// schema-valid, editable document); tests pass `vec![]` when content is
/// irrelevant. The block ids are minted by the caller (an effect) and embedded
/// in the block content as `attributes.blockId`, keeping the builder pure.
///
/// TODO: (templates) when `from_template_id` is supported, the template's
/// blocks are cloned into `seed_blocks` at the call edge — deep-copy each
/// block's content, mint a fresh `BlockId`, **preserve its `section`**, reset
/// the per-section `ordering` — and this sets `template_id` for lineage.
pub fn build_new_page(
    id: PageId,
    name: String,
    status: Status,
    seed_blocks: Vec<NewBlock>,
) -> NewPage {
    NewPage {
        id,
        name,
        status,
        // Everything created today is authored world content. Template creation
        // (the only other current kind) is not wired yet; when it lands it sets
        // `kind: Template` and `template_id` here.
        kind: PageKind::Entity,
        template_id: None,
        blocks: seed_blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_page_has_no_blocks_and_no_template() {
        let id = PageId::generate();
        let new_page = build_new_page(id.clone(), "Korgath".to_string(), Status::GmOnly, vec![]);

        assert_eq!(new_page.id, id);
        assert_eq!(new_page.name, "Korgath");
        assert_eq!(new_page.status, Status::GmOnly);
        assert_eq!(new_page.template_id, None);
        assert!(new_page.blocks.is_empty());
    }

    #[test]
    fn seed_blocks_are_carried_through() {
        let block = NewBlock {
            id: BlockId::generate(),
            section: "body",
            ordering: 0,
            content: b"seed".to_vec(),
            status: Status::GmOnly,
        };
        let nt = build_new_page(
            PageId::generate(),
            "Home".to_string(),
            Status::Known,
            vec![block.clone()],
        );
        assert_eq!(nt.blocks, vec![block]);
    }

    #[test]
    fn status_is_carried_through() {
        for status in [Status::GmOnly, Status::Known, Status::Retconned] {
            let nt = build_new_page(PageId::generate(), "X".to_string(), status, vec![]);
            assert_eq!(nt.status, status);
        }
    }
}
