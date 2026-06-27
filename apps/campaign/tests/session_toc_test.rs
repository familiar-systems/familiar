//! End-to-end: a created session's ToC node carries its kind and ordinal.
//!
//! The supervisor's `CreateSession` workflow mints the temporal row (ordinal =
//! max+1) and places the page in the live ToC via `AddPageNode`. This test
//! drives that path through the registry and then decodes the toc room snapshot
//! the way a joining client does, asserting the session node carries
//! `pageKind == "session"` and the genesis-assigned `ordinal`. That is the data
//! the client composes "Session {ordinal}" from, so this guards the whole
//! threading: ordinal -> AddPageNode -> Loro meta -> snapshot -> client decode.

mod common;

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign::actors::registry::{CreateCampaign, resolve};
use familiar_systems_campaign::actors::supervisor::{CreateSession, JoinRoom};
use familiar_systems_campaign_shared::id::ClientId;
use familiar_systems_campaign_shared::loro::toc::{
    CONTAINER_TOC, KEY_KIND, KEY_ORDINAL, KEY_PAGE_KIND, KIND_PAGE,
};
use loro::{LoroDoc, LoroValue, TreeParentId, ValueOrContainer};
use std::time::Duration;
use tokio::sync::mpsc;

fn as_string(v: Option<ValueOrContainer>) -> Option<String> {
    match v {
        Some(ValueOrContainer::Value(LoroValue::String(s))) => Some(s.to_string()),
        _ => None,
    }
}

fn as_i64(v: Option<ValueOrContainer>) -> Option<i64> {
    match v {
        Some(ValueOrContainer::Value(LoroValue::I64(n))) => Some(n),
        _ => None,
    }
}

#[tokio::test]
async fn session_toc_node_carries_kind_and_ordinal() {
    let app = common::spawn_app().await;
    let campaign_id = CampaignId::generate();
    let state = app
        .registry
        .ask(CreateCampaign {
            campaign_id,
            owner_user_id: common::test_user_id(),
        })
        .await
        .expect("create campaign");
    let supervisor = resolve(Some(state), Duration::from_secs(30))
        .await
        .expect("campaign ready")
        .supervisor;

    // Create a session. CreateSession awaits its AddPageNode, so the node is live
    // in the toc doc by the time the call returns.
    let created = supervisor
        .ask(CreateSession {
            name: "The Gathering Storm".to_string(),
            status: None,
            parent: None,
        })
        .await
        .expect("create session");
    assert_eq!(created.session.ordinal, 1, "first session in the campaign");

    // Join the singleton toc room and decode its snapshot into a raw client doc.
    let handle = supervisor
        .ask(JoinRoom {
            room_id: "toc".to_string(),
        })
        .await
        .expect("join toc room");
    let (tx, _rx) = mpsc::unbounded_channel();
    let resp = handle
        .join(ClientId::new(1), tx, CampaignRole::Gm)
        .await
        .expect("client join");

    let doc = LoroDoc::new();
    doc.import(resp.snapshot.as_bytes())
        .expect("snapshot imports into a fresh doc");
    let tree = doc.get_tree(CONTAINER_TOC);
    let roots = tree
        .children(TreeParentId::Root)
        .expect("toc has root nodes");

    // Find the session page node (the home-base seed is an entity, so filter by
    // pageKind) and assert it carries the ordinal a client renders from.
    let session_ordinal = roots.into_iter().find_map(|id| {
        let meta = tree.get_meta(id).ok()?;
        let is_page = as_string(meta.get(KEY_KIND)).as_deref() == Some(KIND_PAGE);
        let is_session = as_string(meta.get(KEY_PAGE_KIND)).as_deref() == Some("session");
        (is_page && is_session).then(|| as_i64(meta.get(KEY_ORDINAL)))
    });

    assert_eq!(
        session_ordinal,
        Some(Some(1)),
        "the session node is present and carries pageKind=session with ordinal=1"
    );
}
