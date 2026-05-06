//! Round-trip property: anything `BatchFragmenter` produces, `BatchAssembler`
//! reassembles into the original payload byte-for-byte.

use familiar_systems_campaign::wire::assembler::{AssembledBatch, BatchAssembler};
use familiar_systems_campaign::wire::fragmenter::{BatchFragmenter, FragmentPlan};
use familiar_systems_campaign_shared::id::ClientId;
use loro_protocol::BatchId;

const FRAGMENT_SIZE: usize = 100;

fn cid(n: u64) -> ClientId {
    ClientId::new(n)
}

fn bid(n: u8) -> BatchId {
    let mut bytes = [0u8; 8];
    bytes[7] = n;
    BatchId(bytes)
}

/// Make a payload that's distinguishable (each byte = `(i % 251) as u8`)
/// so byte-equality after round-trip is a meaningful assertion.
fn payload_of(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn round_trip(payload: Vec<u8>) -> Vec<u8> {
    let fragmenter = BatchFragmenter::new(FRAGMENT_SIZE);
    let mut assembler = BatchAssembler::new();
    let client = cid(1);
    let batch_id = bid(1);

    match fragmenter.fragment(payload) {
        FragmentPlan::Single { update } => {
            // Per the protocol, an unfragmented update arrives in a
            // DocUpdate frame as one element of `updates: Vec<Vec<u8>>`.
            // The assembler isn't involved on that path; the room sees
            // `&[update]` directly. To match the round-trip contract for
            // this test, we just confirm `update` is the payload.
            update
        }
        FragmentPlan::Many {
            total_size_bytes,
            fragments,
        } => {
            assembler
                .start(client, batch_id, fragments.len() as u64, total_size_bytes)
                .expect("start");
            let mut assembled: Option<AssembledBatch> = None;
            for (index, fragment) in fragments.into_iter().enumerate() {
                let result = assembler
                    .add(client, batch_id, index as u64, fragment)
                    .expect("add");
                if let Some(b) = result {
                    assembled = Some(b);
                }
            }
            assembled.expect("batch completed").payload
        }
    }
}

#[test]
fn empty_payload() {
    assert_eq!(round_trip(Vec::new()), Vec::<u8>::new());
}

#[test]
fn one_byte_under_threshold() {
    let p = payload_of(FRAGMENT_SIZE - 1);
    assert_eq!(round_trip(p.clone()), p);
}

#[test]
fn exactly_at_threshold() {
    let p = payload_of(FRAGMENT_SIZE);
    assert_eq!(round_trip(p.clone()), p);
}

#[test]
fn one_byte_over_threshold() {
    let p = payload_of(FRAGMENT_SIZE + 1);
    assert_eq!(round_trip(p.clone()), p);
}

#[test]
fn evenly_divisible_above_threshold() {
    let p = payload_of(FRAGMENT_SIZE * 3);
    assert_eq!(round_trip(p.clone()), p);
}

#[test]
fn with_remainder_above_threshold() {
    let p = payload_of(FRAGMENT_SIZE * 3 + 7);
    assert_eq!(round_trip(p.clone()), p);
}

#[test]
fn many_fragments() {
    let p = payload_of(FRAGMENT_SIZE * 50 + 13);
    assert_eq!(round_trip(p.clone()), p);
}

#[test]
fn out_of_order_delivery_round_trips() {
    let fragmenter = BatchFragmenter::new(FRAGMENT_SIZE);
    let mut assembler = BatchAssembler::new();
    let payload = payload_of(FRAGMENT_SIZE * 4 + 17);

    let (total_size, fragments) = match fragmenter.fragment(payload.clone()) {
        FragmentPlan::Many {
            total_size_bytes,
            fragments,
        } => (total_size_bytes, fragments),
        FragmentPlan::Single { .. } => panic!("expected Many for this size"),
    };

    assembler
        .start(cid(1), bid(1), fragments.len() as u64, total_size)
        .unwrap();

    // Deliver in reverse order, with the first fragment delivered last.
    let mut indexed: Vec<(usize, Vec<u8>)> = fragments.into_iter().enumerate().collect();
    indexed.reverse();
    let mut out: Option<Vec<u8>> = None;
    for (index, fragment) in indexed {
        let result = assembler
            .add(cid(1), bid(1), index as u64, fragment)
            .unwrap();
        if let Some(batch) = result {
            out = Some(batch.payload);
        }
    }
    assert_eq!(out.unwrap(), payload);
}
