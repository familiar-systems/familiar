//! WebSocket upgrade handler at `GET /campaign/{id}/ws`.
//!
//! Authenticates via Hanko (token in `?token=` query param) and checks
//! campaign membership via the platform's internal API. Both happen once
//! at upgrade time; the resulting [`ConnectionIdentity`] is carried
//! through the connection's lifetime.

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Response;
use fs_id::Nanoid;
use serde::Deserialize;

use familiar_systems_app_shared::id::{CampaignId, UserId};

use crate::actors::registry::EnsureCampaign;
use crate::state::AppState;

use super::connection::{self, ConnectionIdentity};

#[derive(Deserialize)]
pub struct WsAuthParams {
    token: String,
}

pub async fn ws_upgrade(
    Path(campaign_id): Path<String>,
    Query(auth): Query<WsAuthParams>,
    State(state): State<AppState>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    // Step 1: validate the Hanko session token.
    let claims = state.validator.validate(&auth.token).await.map_err(|e| {
        tracing::debug!(error = %e, "ws upgrade: auth rejected");
        StatusCode::UNAUTHORIZED
    })?;

    let user_id = UserId(claims.subject);
    let campaign_id = CampaignId::from(Nanoid::from(campaign_id.clone()));

    // Step 2: check campaign membership on the platform tier.
    let role = state
        .platform_internal
        .check_membership(&campaign_id.0.0, &user_id)
        .await
        .map_err(|e| {
            tracing::warn!(
                campaign_id = %campaign_id.0,
                user_id = %user_id.0,
                error = %e,
                "ws upgrade: membership check failed"
            );
            StatusCode::SERVICE_UNAVAILABLE
        })?
        .ok_or_else(|| {
            tracing::debug!(
                campaign_id = %campaign_id.0,
                user_id = %user_id.0,
                "ws upgrade: user is not a member"
            );
            StatusCode::FORBIDDEN
        })?;

    // Step 3: ensure the campaign is loaded on this shard.
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

    let identity = ConnectionIdentity { user_id, role };
    let client_id = connection::mint_client_id();
    tracing::debug!(
        campaign_id = %campaign_id.0,
        client_id = client_id.0,
        user_id = %identity.user_id.0,
        role = ?identity.role,
        "upgrading websocket"
    );

    Ok(ws.on_upgrade(move |socket| connection::run(socket, client_id, supervisor, identity)))
}
