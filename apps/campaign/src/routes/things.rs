//! `POST /campaign/{id}/things` -- create a Thing.
//!
//! GM-only. The handler is the imperative shell: it authenticates, authorizes
//! (campaign membership with the `Gm` role, checked on the platform tier), and
//! hands a `CreateThing` command to the campaign supervisor. The supervisor
//! spawns the owning `ThingActor`, which persists the Thing's genesis and
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
use familiar_systems_campaign_shared::document::things::{CreateThingRequest, ThingResponse};
use familiar_systems_campaign_shared::id::ThingId;
use fs_id::Nanoid;

use crate::actors::registry::GetCampaign;
use crate::actors::supervisor::{CreateThing, CreateThingError};
use crate::middleware::auth::AuthenticatedUser;
use crate::state::AppState;

#[utoipa::path(
    post,
    path = "/campaign/{id}/things",
    tag = "things",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    request_body = CreateThingRequest,
    responses(
        (status = CREATED, description = "Thing created", body = ThingResponse),
        // 4XX
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        // 5XX
        (status = UNPROCESSABLE_ENTITY, description = "Parent thing not found in the table of contents"),
        (status = NOT_IMPLEMENTED, description = "Creating from a template is not yet supported"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Creation failed"),
    ),
)]
pub async fn create_thing(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<CreateThingRequest>,
) -> impl IntoResponse {
    // Templates do not exist yet; refuse rather than store a dangling lineage.
    if req.from_template_id.is_some() {
        return (
            StatusCode::NOT_IMPLEMENTED,
            "Creating a Thing from a template is not yet supported.",
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
            tracing::warn!(error = %e, "membership check failed during create_thing");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    }

    match supervisor
        .ask(CreateThing {
            name: req.name,
            status: req.status,
            parent: req.parent,
        })
        .await
    {
        Ok(model) => {
            let resp = ThingResponse {
                id: ThingId::from(model.id),
                name: model.name,
                status: model.status.into(),
                prototype_id: model.prototype_id.map(ThingId::from),
                created_at: model.created_at.to_rfc3339(),
                updated_at: model.updated_at.to_rfc3339(),
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(SendError::HandlerError(CreateThingError::ParentNotFound)) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "Parent thing not found in the table of contents.",
        )
            .into_response(),
        Err(SendError::HandlerError(e)) => {
            tracing::error!(error = %e, "create thing failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, "supervisor unreachable during create thing");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}
