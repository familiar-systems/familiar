use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::extract::ws::{self, WebSocket};
use axum::extract::{Path, Query, State, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use familiar_systems_app_shared::id::CampaignId;
use fs_id::Nanoid;

use crate::actors::registry::GetCampaign;
use crate::actors::supervisor::{ClientDisconnected, JoinRoom, JoinRoomResult};
use crate::actors::RoomHandle;
use crate::state::AppState;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Deserialize)]
pub struct WsQueryParams {
    token: String,
}

pub async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Query(params): Query<WsQueryParams>,
) -> impl IntoResponse {
    let claims = match state.validator.validate(&params.token).await {
        Ok(c) => c,
        Err(_) => return StatusCode::UNAUTHORIZED.into_response(),
    };

    let cid = CampaignId::from(Nanoid::from(campaign_id.clone()));
    let supervisor = match state.registry.ask(GetCampaign(cid)).await {
        Ok(Some(sup)) => sup,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    info!(
        campaign_id = %campaign_id,
        user_id = %claims.subject,
        "upgrading websocket"
    );

    ws.on_upgrade(move |socket| handle_ws(socket, supervisor, state))
}

async fn handle_ws(
    socket: WebSocket,
    supervisor: kameo::actor::ActorRef<crate::actors::supervisor::CampaignSupervisor>,
    _state: AppState,
) {
    let (ws_sink, mut ws_stream) = socket.split();
    let conn_id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::unbounded_channel::<ws::Message>();
    let pong_tx = tx.clone();

    info!(conn_id, "websocket connected");

    let mut ws_sink = ws_sink;
    let writer = tokio::spawn(async move {
        let mut rx = rx;
        while let Some(msg) = rx.recv().await {
            if ws_sink.send(msg).await.is_err() {
                break;
            }
        }
        debug!(conn_id, "writer task ended");
    });

    let mut rooms: HashMap<String, RoomHandle> = HashMap::new();

    while let Some(result) = ws_stream.next().await {
        match result {
            Ok(ws::Message::Binary(data)) => {
                let decoded = match crate::protocol::decode_frame(&data) {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(conn_id, error = %e, "failed to decode frame");
                        continue;
                    }
                };

                match decoded {
                    loro_protocol::ProtocolMessage::JoinRequest { room_id, .. } => {
                        info!(conn_id, room_id, "JoinRequest");
                        match supervisor
                            .ask(JoinRoom {
                                conn_id,
                                room_id: room_id.clone(),
                                tx: tx.clone(),
                            })
                            .await
                        {
                            Ok(JoinRoomResult::Joined(handle)) => {
                                rooms.insert(room_id, handle);
                            }
                            Ok(JoinRoomResult::NotFound) => {
                                let error =
                                    crate::protocol::join_error(&room_id, "room not found");
                                let bytes = crate::protocol::encode_message(&error);
                                let _ = tx.send(ws::Message::Binary(bytes.into()));
                            }
                            Ok(JoinRoomResult::ActorError(e)) => {
                                warn!(conn_id, room_id, error = %e, "JoinRoom error");
                                let error = crate::protocol::join_error(&room_id, &e);
                                let bytes = crate::protocol::encode_message(&error);
                                let _ = tx.send(ws::Message::Binary(bytes.into()));
                            }
                            Err(e) => {
                                warn!(conn_id, room_id, error = %e, "supervisor unreachable");
                            }
                        }
                    }
                    loro_protocol::ProtocolMessage::DocUpdate {
                        room_id, updates, ..
                    } => {
                        if let Some(handle) = rooms.get(&room_id) {
                            handle.apply_updates(conn_id, updates).await;
                        } else {
                            warn!(conn_id, room_id, "DocUpdate for unknown room");
                        }
                    }
                    loro_protocol::ProtocolMessage::Leave { room_id, .. } => {
                        if let Some(handle) = rooms.remove(&room_id) {
                            handle.leave(conn_id).await;
                        }
                        debug!(conn_id, room_id, "client left room");
                    }
                    _ => {
                        debug!(conn_id, "ignoring unhandled message type");
                    }
                }
            }
            Ok(ws::Message::Text(text)) => {
                let frame = crate::protocol::TextFrame::parse(text.as_str());
                if let Some(response) = frame.response() {
                    let _ = pong_tx.send(ws::Message::Text(response.into()));
                }
            }
            Ok(ws::Message::Close(_)) => {
                debug!(conn_id, "recv close frame");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                warn!(conn_id, error = %e, "websocket read error");
                break;
            }
        }
    }

    for (_room_id, handle) in rooms.drain() {
        handle.leave(conn_id).await;
    }
    let _ = supervisor
        .tell(ClientDisconnected { conn_id })
        .send()
        .await;
    writer.abort();
    info!(conn_id, "websocket disconnected");
}
