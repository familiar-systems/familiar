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

use familiar_systems_campaign_shared::document::sessions::{CreateSessionRequest, SessionResponse};
use familiar_systems_campaign_shared::id::{PageId, SessionId};

use crate::actors::supervisor::{CreateSession, CreateSessionError};
use crate::middleware::auth::{AuthenticatedUser, authorize_gm};
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
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };

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
