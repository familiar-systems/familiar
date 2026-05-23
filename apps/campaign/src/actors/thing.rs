use std::collections::HashMap;

use axum::extract::ws;
use familiar_systems_campaign_shared::id::ThingId;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::Context;
use kameo::Actor;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::domain::crdt::doc::CrdtDoc;
use crate::loro::thing::LoroThingDoc;

/// Thin-shell ThingActor: holds a LoroThingDoc and handles CRDT sync.
///
/// In this slice the doc is ephemeral (no persistence, no debounce).
/// The actor proves the room-join/update/broadcast plumbing works.
/// Persistence and editor content land in a follow-up slice.
pub struct ThingActor {
    thing_id: ThingId,
    doc: LoroThingDoc,
    subscribers: HashMap<u64, mpsc::UnboundedSender<ws::Message>>,
}

pub struct ThingActorArgs {
    pub thing_id: ThingId,
}

impl Actor for ThingActor {
    type Args = ThingActorArgs;
    type Error = std::convert::Infallible;

    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        info!(thing_id = %args.thing_id.0, "thing actor started (ephemeral doc)");
        Ok(ThingActor {
            thing_id: args.thing_id,
            doc: LoroThingDoc::new(),
            subscribers: HashMap::new(),
        })
    }

    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        info!(thing_id = %self.thing_id.0, ?reason, "thing actor stopping");
        Ok(())
    }
}

impl ThingActor {
    fn send_to(&self, conn_id: u64, data: Vec<u8>) {
        if let Some(tx) = self.subscribers.get(&conn_id)
            && let Err(e) = tx.send(ws::Message::Binary(data.into()))
        {
            warn!(conn_id, error = %e, "failed to send to thing subscriber");
        }
    }
}

// --- CRDT sync messages ---

pub struct ClientJoin {
    pub conn_id: u64,
    pub tx: mpsc::UnboundedSender<ws::Message>,
}

impl kameo::message::Message<ClientJoin> for ThingActor {
    type Reply = ();

    async fn handle(&mut self, msg: ClientJoin, _ctx: &mut Context<Self, Self::Reply>) {
        let room_id = self.thing_id.0.as_str();
        self.subscribers.insert(msg.conn_id, msg.tx);
        debug!(conn_id = msg.conn_id, thing_id = room_id, "client joined thing room");

        let version = self.doc.version();
        let join_ok = crate::protocol::join_response_ok(room_id, version.0);
        let bytes = crate::protocol::encode_message(&join_ok);
        self.send_to(msg.conn_id, bytes);

        let snapshot = self
            .doc
            .export_snapshot()
            .expect("failed to export thing snapshot for join");
        let doc_update = crate::protocol::doc_update(room_id, vec![snapshot.0]);
        let bytes = crate::protocol::encode_message(&doc_update);
        self.send_to(msg.conn_id, bytes);
    }
}

pub struct ClientLeave {
    pub conn_id: u64,
}

impl kameo::message::Message<ClientLeave> for ThingActor {
    type Reply = ();

    async fn handle(&mut self, msg: ClientLeave, _ctx: &mut Context<Self, Self::Reply>) {
        self.subscribers.remove(&msg.conn_id);
        debug!(conn_id = msg.conn_id, thing_id = %self.thing_id.0, "client left thing room");
    }
}

pub struct ApplyUpdates {
    pub conn_id: u64,
    pub updates: Vec<Vec<u8>>,
}

impl kameo::message::Message<ApplyUpdates> for ThingActor {
    type Reply = ();

    async fn handle(&mut self, msg: ApplyUpdates, _ctx: &mut Context<Self, Self::Reply>) {
        let room_id = self.thing_id.0.as_str();
        if let Err(e) = self.doc.apply_updates(&msg.updates) {
            warn!(conn_id = msg.conn_id, thing_id = room_id, error = %e, "failed to apply thing updates");
            return;
        }

        let broadcast_data = crate::protocol::encode_message(&crate::protocol::doc_update(
            room_id,
            msg.updates,
        ));
        let ws_msg = ws::Message::Binary(broadcast_data.into());
        for (&conn_id, tx) in &self.subscribers {
            if conn_id == msg.conn_id {
                continue;
            }
            if let Err(e) = tx.send(ws_msg.clone()) {
                warn!(conn_id, error = %e, "failed to broadcast thing update");
            }
        }
    }
}
