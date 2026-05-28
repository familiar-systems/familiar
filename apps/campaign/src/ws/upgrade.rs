//! WebSocket upgrade handler at `GET /campaign/{id}/ws`.

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::Response;
use fs_id::Nanoid;

use familiar_systems_app_shared::id::CampaignId;

use crate::actors::registry::EnsureCampaign;
use crate::state::AppState;

use super::connection;

pub async fn ws_upgrade(
    Path(campaign_id): Path<String>,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    let campaign_id = CampaignId::from(Nanoid::from(campaign_id.clone()));

    let supervisor = state
        .registry
        .ask(EnsureCampaign {
            campaign_id: campaign_id.clone(),
        })
        .await
        .map_err(|e| {
            tracing::warn!(
                campaign_id = %campaign_id.0,
                error = %e,
                "ws upgrade failed: could not ensure campaign"
            );
            StatusCode::SERVICE_UNAVAILABLE
        })?;

    let client_id = connection::mint_client_id();
    tracing::debug!(
        campaign_id = %campaign_id.0,
        client_id = client_id.0,
        "upgrading websocket"
    );

    Ok(ws.on_upgrade(move |socket| connection::run(socket, client_id, supervisor)))
}
