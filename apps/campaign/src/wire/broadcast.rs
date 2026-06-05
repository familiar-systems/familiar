//! Encode CRDT updates into loro-protocol wire frames for broadcasting
//! to room subscribers.
//!
//! Room actors call [`encode_broadcast`] after `Room::apply_updates` to
//! turn the `Broadcast.updates` payload into encoded `ProtocolMessage`
//! frames that `Room::fan_out` can deliver to subscriber channels.

use std::sync::atomic::{AtomicU64, Ordering};

use loro_protocol::{BatchId, CrdtType, ProtocolMessage, encode};

use super::fragmenter::{BatchFragmenter, FragmentPlan};

static BATCH_COUNTER: AtomicU64 = AtomicU64::new(1);

fn mint_batch_id() -> BatchId {
    let n = BATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
    BatchId(n.to_be_bytes())
}

/// Encode CRDT update bytes into wire-ready `ProtocolMessage` frames,
/// fragmenting oversized payloads per the loro-protocol spec.
///
/// Returns encoded binary frames suitable for [`Room::fan_out`](crate::domain::crdt::room::Room::fan_out).
/// Each frame is a complete loro-protocol binary message.
pub fn encode_broadcast(
    crdt: CrdtType,
    room_id: &str,
    updates: &[Vec<u8>],
    fragmenter: &BatchFragmenter,
) -> Vec<Vec<u8>> {
    let total_size: usize = updates.iter().map(|u| u.len()).sum();

    if total_size == 0 {
        return Vec::new();
    }

    // If all updates fit in a single DocUpdate frame, send them together.
    if total_size <= fragmenter.fragment_size() {
        let batch_id = mint_batch_id();
        let msg = ProtocolMessage::DocUpdate {
            crdt,
            room_id: room_id.to_string(),
            updates: updates.to_vec(),
            batch_id,
        };
        return vec![encode(&msg).expect("encode DocUpdate")];
    }

    // Oversized: concatenate all updates into one payload, fragment it,
    // then encode as DocUpdateFragmentHeader + N DocUpdateFragment.
    let mut payload = Vec::with_capacity(total_size);
    for u in updates {
        payload.extend_from_slice(u);
    }

    match fragmenter.fragment(payload) {
        FragmentPlan::Single { update } => {
            let batch_id = mint_batch_id();
            let msg = ProtocolMessage::DocUpdate {
                crdt,
                room_id: room_id.to_string(),
                updates: vec![update],
                batch_id,
            };
            vec![encode(&msg).expect("encode DocUpdate")]
        }
        FragmentPlan::Many {
            total_size_bytes,
            fragments,
        } => {
            let batch_id = mint_batch_id();
            let mut frames = Vec::with_capacity(1 + fragments.len());

            frames.push(
                encode(&ProtocolMessage::DocUpdateFragmentHeader {
                    crdt,
                    room_id: room_id.to_string(),
                    batch_id,
                    fragment_count: fragments.len() as u64,
                    total_size_bytes,
                })
                .expect("encode DocUpdateFragmentHeader"),
            );

            for (i, fragment) in fragments.into_iter().enumerate() {
                frames.push(
                    encode(&ProtocolMessage::DocUpdateFragment {
                        crdt,
                        room_id: room_id.to_string(),
                        batch_id,
                        index: i as u64,
                        fragment,
                    })
                    .expect("encode DocUpdateFragment"),
                );
            }

            frames
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loro_protocol::decode;

    fn fragmenter() -> BatchFragmenter {
        BatchFragmenter::new(250 * 1024)
    }

    fn small_fragmenter() -> BatchFragmenter {
        BatchFragmenter::new(10)
    }

    #[test]
    fn empty_updates_produces_no_frames() {
        let frames = encode_broadcast(CrdtType::Loro, "toc", &[], &fragmenter());
        assert!(frames.is_empty());
    }

    #[test]
    fn small_update_produces_single_doc_update() {
        let updates = vec![b"hello".to_vec()];
        let frames = encode_broadcast(CrdtType::Loro, "toc", &updates, &fragmenter());
        assert_eq!(frames.len(), 1);

        let msg = decode(&frames[0]).unwrap();
        match msg {
            ProtocolMessage::DocUpdate {
                crdt,
                room_id,
                updates: decoded_updates,
                ..
            } => {
                assert_eq!(crdt, CrdtType::Loro);
                assert_eq!(room_id, "toc");
                assert_eq!(decoded_updates, vec![b"hello".to_vec()]);
            }
            other => panic!("expected DocUpdate, got {other:?}"),
        }
    }

    #[test]
    fn multiple_small_updates_batched_in_single_frame() {
        let updates = vec![b"aaa".to_vec(), b"bbb".to_vec()];
        let frames = encode_broadcast(CrdtType::Loro, "toc", &updates, &fragmenter());
        assert_eq!(frames.len(), 1);

        match decode(&frames[0]).unwrap() {
            ProtocolMessage::DocUpdate { updates, .. } => {
                assert_eq!(updates.len(), 2);
            }
            other => panic!("expected DocUpdate, got {other:?}"),
        }
    }

    #[test]
    fn oversized_update_produces_fragment_header_and_fragments() {
        let payload = vec![0xAB; 25];
        let updates = vec![payload];
        let frames = encode_broadcast(CrdtType::Loro, "toc", &updates, &small_fragmenter());

        assert!(frames.len() > 1, "should produce header + fragments");

        match decode(&frames[0]).unwrap() {
            ProtocolMessage::DocUpdateFragmentHeader {
                fragment_count,
                total_size_bytes,
                ..
            } => {
                assert_eq!(total_size_bytes, 25);
                assert_eq!(fragment_count, 3);
            }
            other => panic!("expected DocUpdateFragmentHeader, got {other:?}"),
        }

        for (i, frame) in frames[1..].iter().enumerate() {
            match decode(frame).unwrap() {
                ProtocolMessage::DocUpdateFragment { index, .. } => {
                    assert_eq!(index, i as u64);
                }
                other => panic!("expected DocUpdateFragment, got {other:?}"),
            }
        }
    }

    #[test]
    fn batch_ids_are_unique() {
        let a = mint_batch_id();
        let b = mint_batch_id();
        assert_ne!(a, b);
    }
}
