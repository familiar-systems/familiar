use std::collections::HashMap;

use axum::extract::ws;
use kameo::actor::{ActorRef, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::Context;
use kameo::Actor;
use loro::TreeID;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::domain::crdt::doc::{CrdtDoc, Snapshot};
use crate::loro::toc::LoroTocDoc;
use familiar_systems_campaign_shared::loro::toc::TocEntry;

use super::database_writer::{DatabaseActor, SaveTocSnapshot};

const TOC_ROOM_ID: &str = "toc";
const PERSIST_DEBOUNCE: std::time::Duration = std::time::Duration::from_secs(2);

pub struct TocActor {
    doc: LoroTocDoc,
    subscribers: HashMap<u64, mpsc::UnboundedSender<ws::Message>>,
    db_writer: ActorRef<DatabaseActor>,
    self_ref: ActorRef<TocActor>,
    dirty: bool,
    persist_timer: Option<tokio::task::JoinHandle<()>>,
}

pub struct TocActorArgs {
    pub snapshot: Option<Snapshot>,
    pub db_writer: ActorRef<DatabaseActor>,
}

impl Actor for TocActor {
    type Args = TocActorArgs;
    type Error = std::convert::Infallible;

    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let doc = match &args.snapshot {
            Some(snapshot) => {
                let doc =
                    LoroTocDoc::from_snapshot(snapshot).expect("failed to import toc snapshot");
                let tree = doc.read_tree();
                info!(entries = tree.len(), "restored toc from snapshot");
                doc
            }
            None => {
                info!("created fresh toc (no snapshot)");
                LoroTocDoc::new()
            }
        };

        Ok(TocActor {
            doc,
            subscribers: HashMap::new(),
            db_writer: args.db_writer,
            self_ref: actor_ref,
            dirty: false,
            persist_timer: None,
        })
    }

    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        info!(?reason, "toc actor stopping");
        if let Some(handle) = self.persist_timer.take() {
            handle.abort();
        }
        if self.dirty {
            self.persist().await;
        }
        Ok(())
    }
}

impl TocActor {
    fn schedule_persist(&mut self) {
        if let Some(handle) = self.persist_timer.take() {
            handle.abort();
        }
        let self_ref = self.self_ref.clone();
        self.persist_timer = Some(tokio::spawn(async move {
            tokio::time::sleep(PERSIST_DEBOUNCE).await;
            let _ = self_ref.tell(PersistNow).send().await;
        }));
    }

    async fn persist(&mut self) {
        let snapshot = self
            .doc
            .export_snapshot()
            .expect("failed to export toc snapshot");
        debug!(
            snapshot_bytes = snapshot.as_bytes().len(),
            "persisting toc snapshot"
        );
        if let Err(e) = self.db_writer.ask(SaveTocSnapshot { snapshot }).await {
            warn!(error = %e, "failed to persist toc snapshot");
        }
        self.dirty = false;
    }

    fn send_to(&self, conn_id: u64, data: Vec<u8>) {
        if let Some(tx) = self.subscribers.get(&conn_id)
            && let Err(e) = tx.send(ws::Message::Binary(data.into()))
        {
            warn!(conn_id, error = %e, "failed to send to toc subscriber");
        }
    }

    fn broadcast_update(&self, update: &[u8], exclude: Option<u64>) {
        if update.is_empty() || self.subscribers.is_empty() {
            return;
        }

        let msg = crate::protocol::doc_update(TOC_ROOM_ID, vec![update.to_vec()]);
        let bytes = crate::protocol::encode_message(&msg);
        let ws_msg = ws::Message::Binary(bytes.into());

        for (&conn_id, tx) in &self.subscribers {
            if Some(conn_id) == exclude {
                continue;
            }
            if let Err(e) = tx.send(ws_msg.clone()) {
                warn!(conn_id, error = %e, "failed to broadcast toc update");
            }
        }
    }
}

// --- CRDT sync messages ---

pub struct ClientJoin {
    pub conn_id: u64,
    pub tx: mpsc::UnboundedSender<ws::Message>,
}

impl kameo::message::Message<ClientJoin> for TocActor {
    type Reply = ();

    async fn handle(&mut self, msg: ClientJoin, _ctx: &mut Context<Self, Self::Reply>) {
        self.subscribers.insert(msg.conn_id, msg.tx);
        debug!(conn_id = msg.conn_id, "client joined toc room");

        let version = self.doc.version();
        let join_ok = crate::protocol::join_response_ok(TOC_ROOM_ID, version.0);
        let bytes = crate::protocol::encode_message(&join_ok);
        self.send_to(msg.conn_id, bytes);

        let snapshot = self
            .doc
            .export_snapshot()
            .expect("failed to export toc snapshot for join");
        let doc_update = crate::protocol::doc_update(TOC_ROOM_ID, vec![snapshot.0]);
        let bytes = crate::protocol::encode_message(&doc_update);
        self.send_to(msg.conn_id, bytes);
    }
}

pub struct ClientLeave {
    pub conn_id: u64,
}

impl kameo::message::Message<ClientLeave> for TocActor {
    type Reply = ();

    async fn handle(&mut self, msg: ClientLeave, _ctx: &mut Context<Self, Self::Reply>) {
        self.subscribers.remove(&msg.conn_id);
        debug!(conn_id = msg.conn_id, "client left toc room");
    }
}

pub struct ApplyUpdates {
    pub conn_id: u64,
    pub updates: Vec<Vec<u8>>,
}

impl kameo::message::Message<ApplyUpdates> for TocActor {
    type Reply = ();

    async fn handle(&mut self, msg: ApplyUpdates, _ctx: &mut Context<Self, Self::Reply>) {
        if let Err(e) = self.doc.apply_updates(&msg.updates) {
            warn!(conn_id = msg.conn_id, error = %e, "failed to apply toc updates");
            return;
        }

        self.dirty = true;
        self.schedule_persist();

        let broadcast_data = crate::protocol::encode_message(&crate::protocol::doc_update(
            TOC_ROOM_ID,
            msg.updates,
        ));
        let ws_msg = ws::Message::Binary(broadcast_data.into());
        for (&conn_id, tx) in &self.subscribers {
            if conn_id == msg.conn_id {
                continue;
            }
            if let Err(e) = tx.send(ws_msg.clone()) {
                warn!(conn_id, error = %e, "failed to broadcast toc update");
            }
        }
    }
}

// --- Server-side mutation messages ---

pub struct AddEntry {
    pub entry: TocEntry,
    pub parent: Option<TreeID>,
}

impl kameo::message::Message<AddEntry> for TocActor {
    type Reply = Result<TreeID, String>;

    async fn handle(
        &mut self,
        msg: AddEntry,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let (delta, tree_id) = self.doc.add_entry(msg.parent, &msg.entry)?;
        info!(title = msg.entry.title(), "added toc entry");
        self.broadcast_update(&delta, None);
        self.dirty = true;
        self.schedule_persist();
        Ok(tree_id)
    }
}

pub struct RemoveEntry {
    pub tree_id: TreeID,
}

impl kameo::message::Message<RemoveEntry> for TocActor {
    type Reply = Result<(), String>;

    async fn handle(
        &mut self,
        msg: RemoveEntry,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let delta = self.doc.remove_entry(msg.tree_id)?;
        info!("removed toc entry");
        self.broadcast_update(&delta, None);
        self.dirty = true;
        self.schedule_persist();
        Ok(())
    }
}

// --- Persistence ---

pub struct PersistNow;

impl kameo::message::Message<PersistNow> for TocActor {
    type Reply = ();

    async fn handle(&mut self, _: PersistNow, _ctx: &mut Context<Self, Self::Reply>) {
        if self.dirty {
            self.persist().await;
        }
    }
}
