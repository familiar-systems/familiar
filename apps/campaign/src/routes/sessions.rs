//! `POST /campaign/{id}/sessions` -- create a session.
//!
//! GM-only. The handler is the imperative shell: it authenticates, authorizes
//! (campaign membership with the `Gm` role, checked on the platform tier), and
//! hands a `CreateSession` command to the campaign supervisor. The supervisor
//! mints the temporal `sessions` row and the Session page together in one
//! genesis transaction (see `CreateSession`).

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use kameo::error::SendError;

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign_shared::document::sessions::{CreateSessionRequest, SessionResponse};
use familiar_systems_campaign_shared::id::{PageId, SessionId};
use fs_id::Nanoid;

use crate::actors::registry::GetCampaign;
use crate::actors::supervisor::{CreateSession, CreateSessionError};
use crate::middleware::auth::AuthenticatedUser;
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/campaign/{id}/sessions",
    tag = "sessions",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    request_body = CreateSessionRequest,
    responses(
        (status = CREATED, description = "Session created", body = SessionResponse),
        // 4XX
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = UNPROCESSABLE_ENTITY, description = "Parent page not found"),
        // 5XX
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Creation failed"),
    ),
)]
pub async fn create_session(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<CreateSessionRequest>,
) -> impl IntoResponse {
    let cid = CampaignId::from(Nanoid::from(campaign_id));

    let supervisor = match state.registry.ask(GetCampaign(cid.clone())).await {
        Ok(Some(sup)) => sup,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    // Authorize: creating a session is a GM action. This is the cross-user
    // boundary, so it must be checked server-side (on the platform tier).
    let caller = UserId(user.id);
    match state
        .platform_internal
        .check_membership(&cid.0.0, &caller)
        .await
    {
        Ok(Some(CampaignRole::Gm)) => {}
        Ok(Some(_)) | Ok(None) => return StatusCode::FORBIDDEN.into_response(),
        Err(e) => {
            tracing::warn!(error = %e, "membership check failed during create_session");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    match supervisor
        .ask(CreateSession {
            name: req.name,
            status: req.status,
            parent: req.parent,
        })
        .await
    {
        Ok(created) => {
            let resp = SessionResponse {
                page_id: PageId::from(created.page.id),
                session_id: SessionId::from(created.session.id),
                ordinal: created.session.ordinal,
                name: created.page.name,
                created_at: created.session.created_at.to_rfc3339(),
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(SendError::HandlerError(CreateSessionError::ParentNotFound)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Parent page not found in the table of contents.",
        )
            .into_response(),
        Err(SendError::HandlerError(e)) => {
            tracing::error!(error = %e, "create session failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "supervisor unreachable during create session");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
