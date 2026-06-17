//! `POST /campaign/{id}/pages` -- create a Page of any kind.
//!
//! GM-only. The handler is the imperative shell: it authenticates, authorizes
//! (campaign membership with the `Gm` role, checked on the platform tier), and
//! dispatches over the request's `kind` discriminant. `entity` and `template`
//! are document pages, minted via the supervisor's `CreatePage`; `session` mints
//! its temporal row too, via `CreateSession`. The supervisor spawns the owning
//! `PageActor`, which persists the Page's genesis and places it in the table of
//! contents. `from_template_id` (entity clone) is accepted but not yet
//! implemented.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kameo::error::SendError;

use familiar_systems_campaign_shared::document::pages::{
    CreatePageRequest, EntityResponse, PageResponse, SessionResponse, TemplateResponse,
};
use familiar_systems_campaign_shared::id::{PageId, SessionId};
use familiar_systems_campaign_shared::page_kind::PageKind;

use crate::actors::supervisor::{CreatePage, CreatePageError, CreateSession, CreateSessionError};
use crate::middleware::auth::{AuthenticatedUser, authorize_gm};
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/campaign/{id}/pages",
    tag = "pages",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    request_body = CreatePageRequest,
    responses(
        (status = CREATED, description = "Page created", body = PageResponse),
        // 4XX
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        // 5XX
        (status = UNPROCESSABLE_ENTITY, description = "Parent page not found, or the page name is empty"),
        (status = NOT_IMPLEMENTED, description = "Creating an entity from a template is not yet supported"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Creation failed"),
    ),
)]
pub async fn create_page(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<CreatePageRequest>,
) -> impl IntoResponse {
    // Cloning an entity from a template is unbuilt; refuse rather than store a
    // dangling lineage. Body-shaped and route-specific, so it precedes
    // authorization (as it always has) - the template clone is refused before
    // the membership probe.
    if let CreatePageRequest::Entity(body) = &req
        && body.from_template_id.is_some()
    {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "Creating a Page from a template is not yet supported.",
        )
            .into_response();
    }

    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };

    // The new Page's sections (and the editable empty paragraph each is seeded
    // with) come from its kind inside the owning PageActor; this handler names
    // the page, picks the kind, and never enumerates sections.
    match req {
        CreatePageRequest::Entity(body) => {
            match supervisor
                .ask(CreatePage {
                    name: body.name,
                    status: body.status,
                    parent: body.parent,
                    kind: PageKind::Entity,
                })
                .await
            {
                Ok(model) => (
                    StatusCode::CREATED,
                    Json(PageResponse::Entity(EntityResponse {
                        id: PageId::from(model.id),
                        name: model.name,
                        status: model.status.into(),
                        template_id: model.template_id.map(PageId::from),
                        created_at: model.created_at.to_rfc3339(),
                        updated_at: model.updated_at.to_rfc3339(),
                    })),
                )
                    .into_response(),
                Err(e) => create_page_error(e),
            }
        }
        CreatePageRequest::Template(body) => {
            match supervisor
                .ask(CreatePage {
                    name: body.name,
                    status: body.status,
                    parent: body.parent,
                    kind: PageKind::Template,
                })
                .await
            {
                Ok(model) => (
                    StatusCode::CREATED,
                    Json(PageResponse::Template(TemplateResponse {
                        id: PageId::from(model.id),
                        name: model.name,
                        status: model.status.into(),
                        created_at: model.created_at.to_rfc3339(),
                        updated_at: model.updated_at.to_rfc3339(),
                    })),
                )
                    .into_response(),
                Err(e) => create_page_error(e),
            }
        }
        CreatePageRequest::Session(body) => {
            match supervisor
                .ask(CreateSession {
                    name: body.name,
                    status: body.status,
                    parent: body.parent,
                })
                .await
            {
                Ok(created) => (
                    StatusCode::CREATED,
                    Json(PageResponse::Session(SessionResponse {
                        page_id: PageId::from(created.page.id),
                        session_id: SessionId::from(created.session.id),
                        ordinal: created.session.ordinal,
                        name: created.page.name,
                        created_at: created.session.created_at.to_rfc3339(),
                    })),
                )
                    .into_response(),
                Err(e) => create_session_error(e),
            }
        }
    }
}

/// Map a document-page (`Entity`/`Template`) creation failure to an HTTP
/// response. Shared by the two document-page arms, which differ only in their
/// success shape.
fn create_page_error(e: SendError<CreatePage, CreatePageError>) -> Response {
    match e {
        SendError::HandlerError(CreatePageError::ParentNotFound) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Parent page not found in the table of contents.",
        )
            .into_response(),
        SendError::HandlerError(CreatePageError::EmptyName) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Page name must not be empty.",
        )
            .into_response(),
        SendError::HandlerError(e) => {
            tracing::error!(error = %e, "create page failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        e => {
            tracing::error!(error = %e, "supervisor unreachable during create page");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Map a session-creation failure to an HTTP response.
fn create_session_error(e: SendError<CreateSession, CreateSessionError>) -> Response {
    match e {
        SendError::HandlerError(CreateSessionError::ParentNotFound) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Parent page not found in the table of contents.",
        )
            .into_response(),
        SendError::HandlerError(e) => {
            tracing::error!(error = %e, "create session failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        e => {
            tracing::error!(error = %e, "supervisor unreachable during create session");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
