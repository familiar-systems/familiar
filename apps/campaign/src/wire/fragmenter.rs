//! `BatchFragmenter` — split a CRDT update payload for the loro-protocol wire.
//!
//! When the server pushes a CRDT update larger than its `fragment_size` cap
//! (typically a hair under 256 KB to leave room for wire-frame overhead),
//! the update must be split per `protocol.md` §"Update Fragments". This is
//! a pure, stateless splitter; the actor that owns the wire connection
//! mints a `BatchId` and emits the resulting `DocUpdate` or
//! `DocUpdateFragmentHeader + N · DocUpdateFragment` frames in order.
//!
//! Pairs with `BatchAssembler` on the receive side; the
//! `wire_roundtrip` integration test verifies symmetry.

#[derive(Debug, Clone, Copy)]
pub struct BatchFragmenter {
    fragment_size: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FragmentPlan {
    /// Send a single `DocUpdate { updates: vec![update], batch_id }`.
    Single { update: Vec<u8> },
    /// Send `DocUpdateFragmentHeader { batch_id, fragment_count,
    /// total_size_bytes }` followed by N `DocUpdateFragment { batch_id,
    /// index, fragment }` frames in order.
    Many {
        total_size_bytes: u64,
        fragments: Vec<Vec<u8>>,
    },
}

impl BatchFragmenter {
    /// `fragment_size` is the cap per fragment in bytes. Should be `< 256
    /// KB` to leave headroom for wire-frame overhead (type byte, room_id,
    /// `BatchId`, varUints). 250 KB is a reasonable default.
    pub fn new(fragment_size: usize) -> Self {
        assert!(fragment_size > 0, "fragment_size must be > 0");
        Self { fragment_size }
    }

    pub fn fragment(&self, update: Vec<u8>) -> FragmentPlan {
        if update.len() <= self.fragment_size {
            return FragmentPlan::Single { update };
        }
        let total_size_bytes = update.len() as u64;
        let mut fragments = Vec::with_capacity(update.len().div_ceil(self.fragment_size));
        let mut remaining = update.as_slice();
        while !remaining.is_empty() {
            let take = remaining.len().min(self.fragment_size);
            let (head, tail) = remaining.split_at(take);
            fragments.push(head.to_vec());
            remaining = tail;
        }
        FragmentPlan::Many {
            total_size_bytes,
            fragments,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_under_threshold_is_single() {
        let f = BatchFragmenter::new(10);
        let plan = f.fragment(b"abcde".to_vec());
        assert_eq!(
            plan,
            FragmentPlan::Single {
                update: b"abcde".to_vec(),
            }
        );
    }

    #[test]
    fn payload_at_threshold_is_single() {
        let f = BatchFragmenter::new(5);
        let plan = f.fragment(b"abcde".to_vec());
        assert_eq!(
            plan,
            FragmentPlan::Single {
                update: b"abcde".to_vec(),
            }
        );
    }

    #[test]
    fn payload_above_threshold_is_many() {
        let f = BatchFragmenter::new(3);
        let plan = f.fragment(b"abcdef".to_vec());
        assert_eq!(
            plan,
            FragmentPlan::Many {
                total_size_bytes: 6,
                fragments: vec![b"abc".to_vec(), b"def".to_vec()],
            }
        );
    }

    #[test]
    fn payload_with_remainder_splits_evenly_with_short_tail() {
        let f = BatchFragmenter::new(3);
        let plan = f.fragment(b"abcdefg".to_vec());
        assert_eq!(
            plan,
            FragmentPlan::Many {
                total_size_bytes: 7,
                fragments: vec![b"abc".to_vec(), b"def".to_vec(), b"g".to_vec()],
            }
        );
    }

    #[test]
    fn payload_one_byte_over_threshold_is_two_chunks() {
        let f = BatchFragmenter::new(5);
        let plan = f.fragment(b"abcdef".to_vec());
        assert_eq!(
            plan,
            FragmentPlan::Many {
                total_size_bytes: 6,
                fragments: vec![b"abcde".to_vec(), b"f".to_vec()],
            }
        );
    }

    #[test]
    fn empty_payload_is_single() {
        let f = BatchFragmenter::new(10);
        let plan = f.fragment(Vec::new());
        assert_eq!(plan, FragmentPlan::Single { update: Vec::new() });
    }

    #[test]
    #[should_panic(expected = "fragment_size must be > 0")]
    fn zero_fragment_size_panics() {
        let _ = BatchFragmenter::new(0);
    }

    #[test]
    fn realistic_size_splits_into_correct_count() {
        let f = BatchFragmenter::new(250 * 1024);
        let payload = vec![0xAB; 1_000_000];
        match f.fragment(payload.clone()) {
            FragmentPlan::Many {
                total_size_bytes,
                fragments,
            } => {
                assert_eq!(total_size_bytes, 1_000_000);
                assert_eq!(fragments.len(), 4);
                assert_eq!(fragments[0].len(), 250 * 1024);
                assert_eq!(fragments[1].len(), 250 * 1024);
                assert_eq!(fragments[2].len(), 250 * 1024);
                assert_eq!(fragments[3].len(), 1_000_000 - 3 * 250 * 1024);
                let total: usize = fragments.iter().map(|f| f.len()).sum();
                assert_eq!(total, 1_000_000);
            }
            FragmentPlan::Single { .. } => panic!("expected Many"),
        }
    }
}
