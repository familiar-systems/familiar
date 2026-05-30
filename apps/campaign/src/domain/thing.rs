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

/// Build the description of a new, empty Thing.
///
/// Pure: no I/O, no clock, no RNG. The id and status are inputs so the function
/// is deterministic and unit-testable. This is the kernel the future AI
/// `create_thing` suggestion path will reuse.
///
/// TODO(templates): when `from_template_id` is supported, this gains a
/// parameter carrying the prototype's blocks and clones them here — deep-copy
/// each block's content, mint a fresh `BlockId`, reset `ordering` — and sets
/// `prototype_id` for lineage. Until then a new Thing is empty; content is
/// added later through the editor as CRDT edits.
pub fn build_new_thing(id: ThingId, name: String, status: Status) -> NewThing {
    NewThing {
        id,
        name,
        status,
        prototype_id: None,
        blocks: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_thing_has_no_blocks_and_no_prototype() {
        let id = ThingId::generate();
        let new_thing = build_new_thing(id.clone(), "Korgath".to_string(), Status::GmOnly);

        assert_eq!(new_thing.id, id);
        assert_eq!(new_thing.name, "Korgath");
        assert_eq!(new_thing.status, Status::GmOnly);
        assert_eq!(new_thing.prototype_id, None);
        assert!(new_thing.blocks.is_empty());
    }

    #[test]
    fn status_is_carried_through() {
        for status in [Status::GmOnly, Status::Known, Status::Retconned] {
            let nt = build_new_thing(ThingId::generate(), "X".to_string(), status);
            assert_eq!(nt.status, status);
        }
    }
}
