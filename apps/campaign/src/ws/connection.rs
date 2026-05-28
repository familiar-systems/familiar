//! WebSocket connection loop: read task + write task, local routing
//! table, loro-protocol dispatch.
//!
//! The read task holds the routing table (`HashMap` keyed by room_id)
//! and the fragment [`BatchAssembler`]. DocUpdate messages route
//! directly to the room actor via the routing table; the supervisor
//! is only consulted for JoinRequest.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::ws::{Message, WebSocket};
use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::UserId;
use familiar_systems_campaign_shared::id::ClientId;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use kameo::actor::ActorRef;
use loro_protocol::{
    BatchId, CrdtType, Permission, ProtocolMessage, RoomErrorCode, UpdateStatusCode, decode, encode,
};
use tokio::sync::mpsc;

use crate::actors::supervisor::{CampaignSupervisor, JoinRoom, RoomHandle};
use crate::wire::assembler::BatchAssembler;
use crate::wire::broadcast::encode_broadcast;
use crate::wire::fragmenter::BatchFragmenter;

/// Identity established at WebSocket upgrade time. Carried through the
/// connection's lifetime. Room joins resolve [`Capability`] from the role
/// here, not from per-JoinRequest auth bytes.
///
/// [`Capability`]: crate::domain::crdt::room_actor::Capability
#[derive(Debug, Clone)]
pub struct ConnectionIdentity {
    pub user_id: UserId,
    pub role: CampaignRole,
}

const FRAGMENT_TIMEOUT: Duration = Duration::from_secs(10);
const BROADCAST_FRAGMENT_SIZE: usize = 250 * 1024;

static NEXT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) fn mint_client_id() -> ClientId {
    ClientId::new(NEXT_CLIENT_ID.fetch_add(1, Ordering::Relaxed))
}

/// Run a WebSocket connection to completion. Spawns a write task and
/// runs the read loop inline. Returns when the connection closes.
pub async fn run(
    socket: WebSocket,
    client_id: ClientId,
    supervisor: ActorRef<CampaignSupervisor>,
    identity: ConnectionIdentity,
) {
    let (ws_write, mut ws_read) = socket.split();

    // Binary frames: room broadcasts + encoded protocol replies.
    let (binary_tx, binary_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    // Text frames: keepalive pong.
    let (text_tx, text_rx) = mpsc::unbounded_channel::<String>();
    // Fragment timeout signals from spawned sleep tasks.
    let (timeout_tx, mut timeout_rx) = mpsc::unbounded_channel::<(ClientId, BatchId)>();

    let write_handle = tokio::spawn(write_loop(ws_write, binary_rx, text_rx));

    let mut rooms: HashMap<String, RoomEntry> = HashMap::new();
    let mut assembler = BatchAssembler::new();
    let fragmenter = BatchFragmenter::new(BROADCAST_FRAGMENT_SIZE);

    loop {
        tokio::select! {
            msg = ws_read.next() => {
                let Some(msg) = msg else { break };
                let msg = match msg {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::debug!(client_id = client_id.0, error = %e, "ws read error");
                        break;
                    }
                };
                match msg {
                    Message::Binary(data) => {
                        let protocol_msg = match decode(&data) {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::debug!(client_id = client_id.0, error = %e, "invalid protocol message");
                                continue;
                            }
                        };
                        handle_protocol_msg(
                            protocol_msg,
                            client_id,
                            &identity,
                            &supervisor,
                            &mut rooms,
                            &mut assembler,
                            &fragmenter,
                            &binary_tx,
                            &timeout_tx,
                        ).await;
                    }
                    Message::Text(ref text) if text.as_str() == "ping" => {
                        let _ = text_tx.send("pong".to_string());
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
            Some((cid, bid)) = timeout_rx.recv() => {
                if assembler.drop_batch(cid, bid) {
                    tracing::debug!(client_id = client_id.0, "fragment timeout, dropping batch");
                    let ack = ProtocolMessage::Ack {
                        crdt: CrdtType::Loro,
                        room_id: String::new(),
                        ref_id: bid,
                        status: UpdateStatusCode::FragmentTimeout,
                    };
                    let _ = send_msg(&binary_tx, &ack);
                }
            }
        }
    }

    for (room_id, entry) in rooms.drain() {
        tracing::debug!(
            client_id = client_id.0,
            room_id,
            "leaving room on disconnect"
        );
        entry.handle.leave(client_id).await;
    }
    assembler.drop_client(client_id);

    drop(binary_tx);
    drop(text_tx);
    let _ = write_handle.await;

    tracing::debug!(client_id = client_id.0, "connection closed");
}

/// Tracks a joined room: the handle for dispatching messages, and the
/// CrdtType from the original JoinRequest (needed to build reply frames).
struct RoomEntry {
    #[allow(dead_code)] // needed when ThingActor lands
    crdt: CrdtType,
    handle: RoomHandle,
}

async fn write_loop(
    mut ws_write: SplitSink<WebSocket, Message>,
    mut binary_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    mut text_rx: mpsc::UnboundedReceiver<String>,
) {
    loop {
        tokio::select! {
            Some(data) = binary_rx.recv() => {
                if ws_write.send(Message::Binary(data.into())).await.is_err() {
                    break;
                }
            }
            Some(text) = text_rx.recv() => {
                if ws_write.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
            else => break,
        }
    }
    let _ = ws_write.close().await;
}

#[allow(clippy::too_many_arguments)]
async fn handle_protocol_msg(
    msg: ProtocolMessage,
    client_id: ClientId,
    identity: &ConnectionIdentity,
    supervisor: &ActorRef<CampaignSupervisor>,
    rooms: &mut HashMap<String, RoomEntry>,
    assembler: &mut BatchAssembler,
    fragmenter: &BatchFragmenter,
    binary_tx: &mpsc::UnboundedSender<Vec<u8>>,
    timeout_tx: &mpsc::UnboundedSender<(ClientId, BatchId)>,
) {
    match msg {
        ProtocolMessage::JoinRequest { crdt, room_id, .. } => {
            handle_join(
                crdt,
                &room_id,
                identity.role,
                client_id,
                supervisor,
                rooms,
                binary_tx,
                fragmenter,
            )
            .await;
        }

        ProtocolMessage::DocUpdate {
            crdt,
            room_id,
            updates,
            batch_id,
        } => {
            handle_doc_update(
                crdt, &room_id, updates, batch_id, client_id, rooms, binary_tx,
            )
            .await;
        }

        ProtocolMessage::DocUpdateFragmentHeader {
            batch_id,
            fragment_count,
            total_size_bytes,
            ..
        } => {
            if let Err(e) = assembler.start(client_id, batch_id, fragment_count, total_size_bytes) {
                tracing::debug!(client_id = client_id.0, error = %e, "fragment header rejected");
                let ack = ProtocolMessage::Ack {
                    crdt: CrdtType::Loro,
                    room_id: String::new(),
                    ref_id: batch_id,
                    status: UpdateStatusCode::InvalidUpdate,
                };
                let _ = send_msg(binary_tx, &ack);
                return;
            }
            let tx = timeout_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(FRAGMENT_TIMEOUT).await;
                let _ = tx.send((client_id, batch_id));
            });
        }

        ProtocolMessage::DocUpdateFragment {
            crdt,
            room_id,
            batch_id,
            index,
            fragment,
        } => match assembler.add(client_id, batch_id, index, fragment) {
            Ok(None) => {}
            Ok(Some(assembled)) => {
                handle_doc_update(
                    crdt,
                    &room_id,
                    vec![assembled.payload],
                    assembled.batch_id,
                    client_id,
                    rooms,
                    binary_tx,
                )
                .await;
            }
            Err(e) => {
                tracing::debug!(client_id = client_id.0, error = %e, "fragment assembly error");
                let ack = ProtocolMessage::Ack {
                    crdt: CrdtType::Loro,
                    room_id,
                    ref_id: batch_id,
                    status: UpdateStatusCode::InvalidUpdate,
                };
                let _ = send_msg(binary_tx, &ack);
            }
        },

        ProtocolMessage::Leave { room_id, .. } => {
            if let Some(entry) = rooms.remove(&room_id) {
                tracing::debug!(client_id = client_id.0, room_id, "client left room");
                entry.handle.leave(client_id).await;
            }
        }

        _ => {
            tracing::trace!(client_id = client_id.0, "ignoring unexpected message type");
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_join(
    crdt: CrdtType,
    room_id: &str,
    role: CampaignRole,
    client_id: ClientId,
    supervisor: &ActorRef<CampaignSupervisor>,
    rooms: &mut HashMap<String, RoomEntry>,
    binary_tx: &mpsc::UnboundedSender<Vec<u8>>,
    fragmenter: &BatchFragmenter,
) {
    let handle = match supervisor
        .ask(JoinRoom {
            room_id: room_id.to_string(),
        })
        .await
    {
        Ok(h) => h,
        Err(e) => {
            tracing::debug!(client_id = client_id.0, room_id, error = %e, "join room dispatch failed");
            let err = ProtocolMessage::JoinError {
                crdt,
                room_id: room_id.to_string(),
                code: loro_protocol::JoinErrorCode::AppError,
                message: e.to_string(),
                receiver_version: None,
                app_code: Some("room_dispatch".to_string()),
            };
            let _ = send_msg(binary_tx, &err);
            return;
        }
    };

    let join_result = handle.join(client_id, binary_tx.clone(), role).await;
    let response = match join_result {
        Ok(r) => r,
        Err(e) => {
            tracing::debug!(client_id = client_id.0, room_id, error = %e, "room join failed");
            let err = ProtocolMessage::JoinError {
                crdt,
                room_id: room_id.to_string(),
                code: loro_protocol::JoinErrorCode::Unknown,
                message: e.to_string(),
                receiver_version: None,
                app_code: None,
            };
            let _ = send_msg(binary_tx, &err);
            return;
        }
    };

    let permission = match response.permission {
        crate::domain::crdt::room_actor::Capability::Read => Permission::Read,
        crate::domain::crdt::room_actor::Capability::Write => Permission::Write,
    };

    let join_ok = ProtocolMessage::JoinResponseOk {
        crdt,
        room_id: room_id.to_string(),
        permission,
        version: response.version.0.clone(),
        extra: None,
    };
    let _ = send_msg(binary_tx, &join_ok);

    // Snapshot backfill: send the full document state to the joining client.
    let snapshot_frames = encode_broadcast(crdt, room_id, &[response.snapshot.0], fragmenter);
    for frame in snapshot_frames {
        let _ = binary_tx.send(frame);
    }

    rooms.insert(room_id.to_string(), RoomEntry { crdt, handle });
    tracing::debug!(client_id = client_id.0, room_id, "client joined room");
}

async fn handle_doc_update(
    crdt: CrdtType,
    room_id: &str,
    updates: Vec<Vec<u8>>,
    batch_id: BatchId,
    client_id: ClientId,
    rooms: &mut HashMap<String, RoomEntry>,
    binary_tx: &mpsc::UnboundedSender<Vec<u8>>,
) {
    let Some(entry) = rooms.get(room_id) else {
        let err = ProtocolMessage::RoomError {
            crdt,
            room_id: room_id.to_string(),
            code: RoomErrorCode::RejoinSuggested,
            message: "not joined to this room".to_string(),
        };
        let _ = send_msg(binary_tx, &err);
        return;
    };

    match entry.handle.update(client_id, updates).await {
        Ok(_ack_payload) => {
            let ack = ProtocolMessage::Ack {
                crdt,
                room_id: room_id.to_string(),
                ref_id: batch_id,
                status: UpdateStatusCode::Ok,
            };
            let _ = send_msg(binary_tx, &ack);
        }
        Err(e) => {
            let is_stale = matches!(
                e,
                crate::domain::crdt::room_actor::UpdateError::Apply(ref msg)
                    if msg.contains("ActorStopped") || msg.contains("mailbox")
            );
            if is_stale {
                tracing::debug!(
                    client_id = client_id.0,
                    room_id,
                    "room actor stale, removing"
                );
                rooms.remove(room_id);
                let err = ProtocolMessage::RoomError {
                    crdt,
                    room_id: room_id.to_string(),
                    code: RoomErrorCode::RejoinSuggested,
                    message: "room actor evicted".to_string(),
                };
                let _ = send_msg(binary_tx, &err);
            } else {
                let status = match e {
                    crate::domain::crdt::room_actor::UpdateError::Unauthorized => {
                        UpdateStatusCode::PermissionDenied
                    }
                    _ => UpdateStatusCode::InvalidUpdate,
                };
                let ack = ProtocolMessage::Ack {
                    crdt,
                    room_id: room_id.to_string(),
                    ref_id: batch_id,
                    status,
                };
                let _ = send_msg(binary_tx, &ack);
            }
        }
    }
}

fn send_msg(
    tx: &mpsc::UnboundedSender<Vec<u8>>,
    msg: &ProtocolMessage,
) -> Result<(), mpsc::error::SendError<Vec<u8>>> {
    let bytes = encode(msg).expect("encode protocol message");
    tx.send(bytes)
}
