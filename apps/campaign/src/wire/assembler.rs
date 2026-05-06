//! `BatchAssembler` — reassemble fragmented loro-protocol updates.
//!
//! A producer sending a CRDT update larger than the protocol's per-message
//! cap (256 KB per `protocol.md` §"Update Fragments") splits it into one
//! `DocUpdateFragmentHeader` followed by N `DocUpdateFragment` frames. The
//! assembler accepts those frames in any order and produces the original
//! payload once all fragments have arrived.
//!
//! This is a pure state machine — no time, no scheduler, no mailbox. The
//! protocol's 10-second reassembly timeout is enforced one layer up by the
//! [`FragmentReassembly`](super::reassembly::FragmentReassembly) trait,
//! which schedules a self-message at the deadline and calls
//! [`drop_batch`](BatchAssembler::drop_batch) when it fires. That split
//! keeps this module testable without a tokio runtime and lets every actor
//! that owns an assembler get reassembly-with-timeout for free.

use std::collections::HashMap;

use familiar_systems_campaign_shared::id::ClientId;
use loro_protocol::BatchId;

/// Maximum bytes per fragment, per `protocol.md` §"Update Fragments".
pub const MAX_FRAGMENT_BYTES: usize = 256 * 1024;

/// Defensive cap on `fragment_count` to prevent unbounded allocation from a
/// malicious header. Realistic CRDT update payloads should never approach
/// this; raise if real workloads require it.
pub const MAX_FRAGMENT_COUNT: u64 = 4096;

#[derive(Debug, Default)]
pub struct BatchAssembler {
    in_flight: HashMap<(ClientId, BatchId), FragmentBuffer>,
}

#[derive(Debug)]
struct FragmentBuffer {
    chunks: Vec<Option<Vec<u8>>>,
    received: u64,
    expected_count: u64,
    declared_size: u64,
    accumulated_size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssembledBatch {
    pub client: ClientId,
    pub batch_id: BatchId,
    pub payload: Vec<u8>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AssembleError {
    #[error("batch already in progress for this client")]
    DuplicateBatch,
    #[error("unknown batch (no header received)")]
    UnknownBatch,
    #[error("duplicate fragment at index {index}")]
    DuplicateFragment { index: u64 },
    #[error("fragment index {index} >= count {count}")]
    IndexOutOfRange { index: u64, count: u64 },
    #[error("fragment size {size} exceeds 256 KB cap")]
    OversizedFragment { size: usize },
    #[error("accumulated bytes {actual} exceeds declared {declared}")]
    OversizedBatch { actual: u64, declared: u64 },
    #[error("fragment_count of zero")]
    EmptyBatch,
    #[error("fragment count {count} exceeds defensive cap {cap}")]
    TooManyFragments { count: u64, cap: u64 },
    #[error("declared size {declared} != assembled length {actual}")]
    SizeMismatch { actual: usize, declared: u64 },
}

impl BatchAssembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Begin a new fragmented batch. Errors leave existing in-flight state
    /// untouched.
    pub fn start(
        &mut self,
        client: ClientId,
        batch_id: BatchId,
        fragment_count: u64,
        total_size_bytes: u64,
    ) -> Result<(), AssembleError> {
        if fragment_count == 0 {
            return Err(AssembleError::EmptyBatch);
        }
        if fragment_count > MAX_FRAGMENT_COUNT {
            return Err(AssembleError::TooManyFragments {
                count: fragment_count,
                cap: MAX_FRAGMENT_COUNT,
            });
        }
        let key = (client, batch_id);
        if self.in_flight.contains_key(&key) {
            return Err(AssembleError::DuplicateBatch);
        }
        // Cast is safe: fragment_count is bounded by MAX_FRAGMENT_COUNT.
        let chunks = vec![None; fragment_count as usize];
        self.in_flight.insert(
            key,
            FragmentBuffer {
                chunks,
                received: 0,
                expected_count: fragment_count,
                declared_size: total_size_bytes,
                accumulated_size: 0,
            },
        );
        Ok(())
    }

    /// Add one fragment. Returns `Ok(Some(batch))` when this fragment
    /// completes the batch, `Ok(None)` if more fragments are expected.
    ///
    /// Protocol-violation errors (anything other than `UnknownBatch`) drop
    /// the in-flight buffer so the actor doesn't accumulate broken state;
    /// the actor responds with the appropriate `Ack { status }` and moves
    /// on. `UnknownBatch` is non-destructive — there's nothing to drop.
    pub fn add(
        &mut self,
        client: ClientId,
        batch_id: BatchId,
        index: u64,
        fragment: Vec<u8>,
    ) -> Result<Option<AssembledBatch>, AssembleError> {
        let key = (client, batch_id);

        if fragment.len() > MAX_FRAGMENT_BYTES {
            self.in_flight.remove(&key);
            return Err(AssembleError::OversizedFragment {
                size: fragment.len(),
            });
        }

        let buf = match self.in_flight.get_mut(&key) {
            Some(b) => b,
            None => return Err(AssembleError::UnknownBatch),
        };

        if index >= buf.expected_count {
            let count = buf.expected_count;
            self.in_flight.remove(&key);
            return Err(AssembleError::IndexOutOfRange { index, count });
        }
        let slot = &mut buf.chunks[index as usize];
        if slot.is_some() {
            self.in_flight.remove(&key);
            return Err(AssembleError::DuplicateFragment { index });
        }

        let fragment_size = fragment.len() as u64;
        let new_accumulated = buf.accumulated_size + fragment_size;
        if new_accumulated > buf.declared_size {
            let declared = buf.declared_size;
            self.in_flight.remove(&key);
            return Err(AssembleError::OversizedBatch {
                actual: new_accumulated,
                declared,
            });
        }

        *slot = Some(fragment);
        buf.accumulated_size = new_accumulated;
        buf.received += 1;

        if buf.received < buf.expected_count {
            return Ok(None);
        }

        let buf = self.in_flight.remove(&key).expect("present, just got_mut");
        if buf.accumulated_size != buf.declared_size {
            return Err(AssembleError::SizeMismatch {
                actual: buf.accumulated_size as usize,
                declared: buf.declared_size,
            });
        }
        let mut payload = Vec::with_capacity(buf.declared_size as usize);
        for chunk in buf.chunks {
            let bytes = chunk.expect("all slots filled (received == expected_count)");
            payload.extend_from_slice(&bytes);
        }
        Ok(Some(AssembledBatch {
            client,
            batch_id,
            payload,
        }))
    }

    /// Drop one batch. Returns `true` if it was in flight (i.e. the caller
    /// should emit a `fragment_timeout` ack), `false` if it had already
    /// completed or never started (a stale timeout-fire — no-op).
    pub fn drop_batch(&mut self, client: ClientId, batch_id: BatchId) -> bool {
        self.in_flight.remove(&(client, batch_id)).is_some()
    }

    /// Drop all in-flight batches for a disconnecting client.
    pub fn drop_client(&mut self, client: ClientId) {
        self.in_flight.retain(|(c, _), _| *c != client);
    }

    pub fn contains(&self, client: ClientId, batch_id: BatchId) -> bool {
        self.in_flight.contains_key(&(client, batch_id))
    }

    #[cfg(test)]
    fn in_flight_len(&self) -> usize {
        self.in_flight.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cid(n: u64) -> ClientId {
        ClientId::new(n)
    }

    fn bid(n: u8) -> BatchId {
        let mut bytes = [0u8; 8];
        bytes[7] = n;
        BatchId(bytes)
    }

    #[test]
    fn single_fragment_completes_immediately() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 1, 5).unwrap();
        let result = a.add(cid(1), bid(1), 0, b"hello".to_vec()).unwrap();
        assert_eq!(
            result,
            Some(AssembledBatch {
                client: cid(1),
                batch_id: bid(1),
                payload: b"hello".to_vec(),
            })
        );
        assert_eq!(a.in_flight_len(), 0);
    }

    #[test]
    fn multi_fragment_in_order_completes() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 3, 9).unwrap();
        assert!(a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap().is_none());
        assert!(a.add(cid(1), bid(1), 1, b"def".to_vec()).unwrap().is_none());
        let r = a.add(cid(1), bid(1), 2, b"ghi".to_vec()).unwrap();
        assert_eq!(r.unwrap().payload, b"abcdefghi");
    }

    #[test]
    fn multi_fragment_reverse_order_completes() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 3, 9).unwrap();
        assert!(a.add(cid(1), bid(1), 2, b"ghi".to_vec()).unwrap().is_none());
        assert!(a.add(cid(1), bid(1), 1, b"def".to_vec()).unwrap().is_none());
        let r = a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap();
        assert_eq!(r.unwrap().payload, b"abcdefghi");
    }

    #[test]
    fn interleaved_clients_complete_independently() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 2, 6).unwrap();
        a.start(cid(2), bid(1), 2, 6).unwrap();
        a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap();
        a.add(cid(2), bid(1), 0, b"xyz".to_vec()).unwrap();
        let r1 = a.add(cid(1), bid(1), 1, b"def".to_vec()).unwrap();
        let r2 = a.add(cid(2), bid(1), 1, b"uvw".to_vec()).unwrap();
        assert_eq!(r1.unwrap().payload, b"abcdef");
        assert_eq!(r2.unwrap().payload, b"xyzuvw");
    }

    #[test]
    fn duplicate_start_rejected() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 2, 6).unwrap();
        let err = a.start(cid(1), bid(1), 2, 6).unwrap_err();
        assert_eq!(err, AssembleError::DuplicateBatch);
        assert_eq!(a.in_flight_len(), 1);
    }

    #[test]
    fn add_without_start_is_unknown_batch() {
        let mut a = BatchAssembler::new();
        let err = a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap_err();
        assert_eq!(err, AssembleError::UnknownBatch);
    }

    #[test]
    fn duplicate_fragment_drops_batch() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 3, 9).unwrap();
        a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap();
        let err = a.add(cid(1), bid(1), 0, b"xyz".to_vec()).unwrap_err();
        assert_eq!(err, AssembleError::DuplicateFragment { index: 0 });
        assert_eq!(a.in_flight_len(), 0);
    }

    #[test]
    fn out_of_range_index_drops_batch() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 2, 6).unwrap();
        let err = a.add(cid(1), bid(1), 5, b"abc".to_vec()).unwrap_err();
        assert_eq!(err, AssembleError::IndexOutOfRange { index: 5, count: 2 });
        assert_eq!(a.in_flight_len(), 0);
    }

    #[test]
    fn oversized_fragment_drops_batch() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 1, MAX_FRAGMENT_BYTES as u64 + 1)
            .unwrap();
        let huge = vec![0u8; MAX_FRAGMENT_BYTES + 1];
        let err = a.add(cid(1), bid(1), 0, huge).unwrap_err();
        assert!(matches!(err, AssembleError::OversizedFragment { .. }));
        assert_eq!(a.in_flight_len(), 0);
    }

    #[test]
    fn oversized_batch_drops() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 2, 6).unwrap();
        a.add(cid(1), bid(1), 0, b"abcd".to_vec()).unwrap();
        let err = a.add(cid(1), bid(1), 1, b"efghij".to_vec()).unwrap_err();
        assert_eq!(
            err,
            AssembleError::OversizedBatch {
                actual: 10,
                declared: 6,
            }
        );
        assert_eq!(a.in_flight_len(), 0);
    }

    #[test]
    fn empty_batch_rejected() {
        let mut a = BatchAssembler::new();
        let err = a.start(cid(1), bid(1), 0, 0).unwrap_err();
        assert_eq!(err, AssembleError::EmptyBatch);
    }

    #[test]
    fn too_many_fragments_rejected() {
        let mut a = BatchAssembler::new();
        let err = a
            .start(cid(1), bid(1), MAX_FRAGMENT_COUNT + 1, 0)
            .unwrap_err();
        assert!(matches!(err, AssembleError::TooManyFragments { .. }));
    }

    #[test]
    fn size_mismatch_on_completion() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 2, 10).unwrap();
        a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap();
        let err = a.add(cid(1), bid(1), 1, b"defg".to_vec()).unwrap_err();
        assert_eq!(
            err,
            AssembleError::SizeMismatch {
                actual: 7,
                declared: 10,
            }
        );
    }

    #[test]
    fn drop_batch_returns_true_when_present() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 2, 6).unwrap();
        a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap();
        assert!(a.drop_batch(cid(1), bid(1)));
        assert_eq!(a.in_flight_len(), 0);
        assert!(!a.contains(cid(1), bid(1)));
    }

    #[test]
    fn drop_batch_returns_false_when_absent() {
        let mut a = BatchAssembler::new();
        assert!(!a.drop_batch(cid(1), bid(1)));
    }

    #[test]
    fn drop_batch_returns_false_for_already_completed_batch() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 1, 3).unwrap();
        a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap();
        // Stale-fire scenario: the timeout message arrives after the
        // batch already completed. The trait's handler should see false
        // here and no-op.
        assert!(!a.drop_batch(cid(1), bid(1)));
    }

    #[test]
    fn contains_reports_in_flight_state() {
        let mut a = BatchAssembler::new();
        assert!(!a.contains(cid(1), bid(1)));
        a.start(cid(1), bid(1), 2, 6).unwrap();
        assert!(a.contains(cid(1), bid(1)));
        a.add(cid(1), bid(1), 0, b"abc".to_vec()).unwrap();
        assert!(a.contains(cid(1), bid(1)));
        a.add(cid(1), bid(1), 1, b"def".to_vec()).unwrap();
        assert!(!a.contains(cid(1), bid(1)));
    }

    #[test]
    fn drop_client_removes_only_that_clients_batches() {
        let mut a = BatchAssembler::new();
        a.start(cid(1), bid(1), 2, 6).unwrap();
        a.start(cid(1), bid(2), 2, 6).unwrap();
        a.start(cid(2), bid(1), 2, 6).unwrap();
        a.drop_client(cid(1));
        assert_eq!(a.in_flight_len(), 1);
        a.add(cid(2), bid(1), 0, b"abc".to_vec()).unwrap();
        let r = a.add(cid(2), bid(1), 1, b"def".to_vec()).unwrap();
        assert_eq!(r.unwrap().payload, b"abcdef");
    }
}
