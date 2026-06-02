//! Server-side half of the ToC empty-backfill investigation.
//!
//! Symptom (dev, React StrictMode): a client connects, the toc room joins,
//! disconnects, and re-joins on a fresh socket; the second join's backfill comes
//! back empty and blanks the sidebar. This test isolates whether the *server*
//! is at fault: does the singleton `TocActor` still hand a full snapshot to a
//! client that joins after a previous client left?
//!
//! The actor mailbox serializes `ClientLeave` before the next `ClientJoin`, so
//! this is the clean, deterministic leave/rejoin case (not the transport-level
//! overlap, which lives in the WS/loro-websocket layer the browser exercises).
//! If this stays green, the server is exonerated and the bug is client/transport.

mod common;

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::registry::CreateCampaign;
use familiar_systems_campaign::actors::supervisor::{CreateThing, JoinRoom};
use familiar_systems_campaign::domain::crdt::doc::{CrdtDoc, Snapshot};
use familiar_systems_campaign::loro::toc::LoroTocDoc;
use familiar_systems_campaign_shared::id::ClientId;
use tokio::sync::mpsc;

/// Decode a room snapshot through the real read path and count root-level nodes.
fn snapshot_root_count(snapshot: &Snapshot) -> usize {
    let mut probe = LoroTocDoc::new();
    probe
        .import_snapshot(snapshot)
        .expect("room snapshot imports cleanly into a fresh doc");
    probe.read_tree().len()
}

#[tokio::test]
async fn toc_rejoin_after_leave_still_gets_full_snapshot() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    let supervisor = app
        .registry
        .ask(CreateCampaign {
            campaign_id,
            owner_user_id: common::test_user_id(),
        })
        .await
        .expect("create campaign");

    // Populate the ToC with two root-level Things. CreateThing awaits its
    // AddThingNode, so each node is live in the doc by the time the call returns.
    for name in ["Korgath", "The Rusted Flagon"] {
        supervisor
            .ask(CreateThing {
                name: name.to_string(),
                status: None,
                parent: None,
                seed_blocks: vec![],
            })
            .await
            .expect("create thing");
    }

    // Client A joins the singleton toc room and sees the populated tree.
    let handle_a = supervisor
        .ask(JoinRoom {
            room_id: "toc".to_string(),
        })
        .await
        .expect("join toc room (A)");
    let (tx_a, _rx_a) = mpsc::unbounded_channel();
    let resp_a = handle_a
        .join(ClientId::new(1), tx_a, CampaignRole::Gm)
        .await
        .expect("client A join");
    let a_count = snapshot_root_count(&resp_a.snapshot);
    assert!(
        a_count >= 2,
        "client A should see the two created Things (plus the async home-base seed), got {a_count}"
    );

    // Client A leaves — mirrors the WS connection's per-room leave on disconnect.
    handle_a.leave(ClientId::new(1)).await;

    // Client B rejoins the same singleton room on a fresh handle / client id.
    let handle_b = supervisor
        .ask(JoinRoom {
            room_id: "toc".to_string(),
        })
        .await
        .expect("join toc room (B)");
    let (tx_b, _rx_b) = mpsc::unbounded_channel();
    let resp_b = handle_b
        .join(ClientId::new(2), tx_b, CampaignRole::Gm)
        .await
        .expect("client B join");
    let b_count = snapshot_root_count(&resp_b.snapshot);

    // The rejoin must not lose nodes. (`>=`, not `==`: the home-base seed may
    // land between the two joins, which only ever increases the count.) The bug
    // under investigation would collapse b_count to 0.
    assert!(
        b_count >= a_count,
        "rejoin lost ToC nodes: A saw {a_count}, B saw {b_count}"
    );
    assert!(
        !resp_b.version.0.is_empty(),
        "rejoin must report a non-empty server version, got empty"
    );
}
