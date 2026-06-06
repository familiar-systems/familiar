//! `POST /campaign/{id}/pages` -- create a Page.
//!
//! GM-only. The handler is the imperative shell: it authenticates, authorizes
//! (campaign membership with the `Gm` role, checked on the platform tier), and
//! hands a `CreatePage` command to the campaign supervisor. The supervisor
//! spawns the owning `PageActor`, which persists the Page's genesis and
//! places it in the table of contents. `from_template_id` is accepted but not
//! yet implemented.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use kameo::error::SendError;

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign_shared::document::pages::{CreatePageRequest, PageResponse};
use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::status::Status;
use fs_id::Nanoid;

use crate::actors::registry::GetCampaign;
use crate::actors::supervisor::{CreatePage, CreatePageError};
use crate::domain::page::NewBlock;
use crate::loro::block_codec;
use crate::middleware::auth::AuthenticatedUser;
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
        (status = UNPROCESSABLE_ENTITY, description = "Parent page not found in the table of contents"),
        (status = NOT_IMPLEMENTED, description = "Creating from a template is not yet supported"),
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
    // Templates do not exist yet; refuse rather than store a dangling lineage.
    if req.from_template_id.is_some() {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "Creating a Page from a template is not yet supported.",
        )
            .into_response();
    }

    let cid = CampaignId::from(Nanoid::from(campaign_id));

    let supervisor = match state.registry.ask(GetCampaign(cid.clone())).await {
        Ok(Some(sup)) => sup,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return StatusCode::SERVICE_UNAVAILABLE.into_response(),
    };

    // Authorize: creating content is a GM action. This is the cross-user
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
            tracing::warn!(error = %e, "membership check failed during create_page");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    // Seed one empty paragraph so the new page opens as a schema-valid, editable
    // ProseMirror document (a `doc` with one `block+` child) rather than an empty,
    // uneditable `doc`. Mirrors the campaign home-page seed; the block's ULID is
    // embedded in `attributes.blockId` and reused as its row id for stable identity.
    let block_id = BlockId::generate();
    let seed_blocks = vec![NewBlock {
        id: block_id.clone(),
        ordering: 0,
        content: block_codec::empty_paragraph_blob(&block_id),
        status: Status::GmOnly,
    }];

    match supervisor
        .ask(CreatePage {
            name: req.name,
            status: req.status,
            parent: req.parent,
            seed_blocks,
        })
        .await
    {
        Ok(model) => {
            let resp = PageResponse {
                id: PageId::from(model.id),
                name: model.name,
                status: model.status.into(),
                kind: model.kind.into(),
                template_id: model.template_id.map(PageId::from),
                created_at: model.created_at.to_rfc3339(),
                updated_at: model.updated_at.to_rfc3339(),
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(SendError::HandlerError(CreatePageError::ParentNotFound)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Parent page not found in the table of contents.",
        )
            .into_response(),
        Err(SendError::HandlerError(e)) => {
            tracing::error!(error = %e, "create page failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "supervisor unreachable during create page");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
