//! Pure builder for bringing a new Thing into existence.
//!
//! This is the "service" in the functional-core / imperative-shell sense: a
//! pure function that composes the domain values to persist, with all effects
//! (id minting, the clock, the database, the live CRDT doc) pushed to the
//! edges. It does NOT write anything. The `ThingActor`'s genesis path consumes
//! the [`NewThing`] it returns: the actor persists it (via `DbCreateThing`) and
//! builds its own `LoroThingDoc`.
//!
//! Keeping creation logic here, and routing the write through the owning actor,
//! is deliberate: a `ThingActor` is the single-threaded consistency boundary
//! for a Thing, so every mutation flows through it. A direct write around the
//! actor would drift its in-memory CRDT doc from the database the moment the
//! Thing has live subscribers.

use familiar_systems_campaign_shared::id::{BlockId, ThingId};
use familiar_systems_campaign_shared::status::Status;

/// A block to persist as part of a new Thing. A neutral domain value with no
/// sea-orm dependency; the `DatabaseWriteActor` maps it to a `blocks::ActiveModel`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBlock {
    pub id: BlockId,
    pub ordering: i64,
    pub content: Vec<u8>,
    pub status: Status,
}

/// The complete description of a Thing to bring into existence. Produced by
/// [`build_new_thing`] and handed to the `ThingActor` genesis path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewThing {
    pub id: ThingId,
    pub name: String,
    pub status: Status,
    /// Lineage back to the template this was cloned from, if any. `None` until
    /// template instantiation lands.
    pub prototype_id: Option<ThingId>,
    pub blocks: Vec<NewBlock>,
}

/// Build the description of a new Thing.
///
/// Pure: no I/O, no clock, no RNG. The id, status, and `seed_blocks` are inputs
/// so the function is deterministic and unit-testable. This is the kernel the
/// future AI `create_thing` suggestion path will reuse.
///
/// `seed_blocks` is the Thing's initial content. Most callers pass `vec![]`
/// (an empty Thing whose content is added later through the editor); the
/// campaign home-page seed passes one empty paragraph so the page opens as a
/// schema-valid, editable document. The block ids are minted by the caller (an
/// effect) and embedded in the block content as `attributes.blockId`, keeping
/// the builder pure.
///
/// TODO(templates): when `from_template_id` is supported, the prototype's
/// blocks are cloned into `seed_blocks` at the call edge — deep-copy each
/// block's content, mint a fresh `BlockId`, reset `ordering` — and this sets
/// `prototype_id` for lineage.
pub fn build_new_thing(
    id: ThingId,
    name: String,
    status: Status,
    seed_blocks: Vec<NewBlock>,
) -> NewThing {
    NewThing {
        id,
        name,
        status,
        prototype_id: None,
        blocks: seed_blocks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_thing_has_no_blocks_and_no_prototype() {
        let id = ThingId::generate();
        let new_thing = build_new_thing(id.clone(), "Korgath".to_string(), Status::GmOnly, vec![]);

        assert_eq!(new_thing.id, id);
        assert_eq!(new_thing.name, "Korgath");
        assert_eq!(new_thing.status, Status::GmOnly);
        assert_eq!(new_thing.prototype_id, None);
        assert!(new_thing.blocks.is_empty());
    }

    #[test]
    fn seed_blocks_are_carried_through() {
        let block = NewBlock {
            id: BlockId::generate(),
            ordering: 0,
            content: b"seed".to_vec(),
            status: Status::GmOnly,
        };
        let nt = build_new_thing(
            ThingId::generate(),
            "Home".to_string(),
            Status::Known,
            vec![block.clone()],
        );
        assert_eq!(nt.blocks, vec![block]);
    }

    #[test]
    fn status_is_carried_through() {
        for status in [Status::GmOnly, Status::Known, Status::Retconned] {
            let nt = build_new_thing(ThingId::generate(), "X".to_string(), status, vec![]);
            assert_eq!(nt.status, status);
        }
    }
}
