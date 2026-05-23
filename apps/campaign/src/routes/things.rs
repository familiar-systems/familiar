//! `POST /campaign/{id}/things` and `DELETE /campaign/{id}/things/{thing_id}`.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign_shared::id::ThingId;
use familiar_systems_campaign_shared::things::{CreateThingRequest, CreateThingResponse};
use fs_id::Nanoid;

use crate::actors::database_writer::GetMetadata;
use crate::actors::registry::GetCampaign;
use crate::actors::supervisor::{CampaignSupervisor, CreateThing, DeleteThing};
use crate::middleware::auth::AuthenticatedUser;
use crate::state::AppState;

/// Look up the supervisor for the given campaign, verify the caller owns it.
/// Returns `(supervisor, campaign_id_str)` on success, or an error response.
async fn resolve_owned_campaign(
    state: &AppState,
    user: &AuthenticatedUser,
    raw_id: &str,
) -> Result<kameo::actor::ActorRef<CampaignSupervisor>, (StatusCode, &'static str)> {
    let cid = CampaignId::from(Nanoid::from(raw_id.to_string()));
    let supervisor = match state.registry.ask(GetCampaign(cid)).await {
        Ok(Some(sup)) => sup,
        Ok(None) => return Err((StatusCode::NOT_FOUND, "Campaign not on this shard")),
        Err(_) => return Err((StatusCode::SERVICE_UNAVAILABLE, "Server is restarting")),
    };

    let meta = supervisor
        .ask(GetMetadata)
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read campaign metadata"))?;

    let caller = UserId(user.id);
    if !meta.is_owned_by(&caller) {
        return Err((StatusCode::FORBIDDEN, "Not the campaign owner"));
    }

    Ok(supervisor)
}

#[utoipa::path(
    post,
    path = "/campaign/{id}/things",
    tag = "things",
    params(
        ("id" = String, Path, description = "Campaign ID"),
    ),
    request_body = CreateThingRequest,
    responses(
        (status = CREATED, description = "Thing created", body = CreateThingResponse),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Not the campaign owner"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = 503, description = "Server restarting"),
        (status = 500, description = "Internal error"),
    ),
)]
pub async fn create_thing(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<CreateThingRequest>,
) -> impl IntoResponse {
    let supervisor = match resolve_owned_campaign(&state, &user, &campaign_id).await {
        Ok(sup) => sup,
        Err((status, msg)) => {
            return (status, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };

    match supervisor.ask(CreateThing { name: req.name }).await {
        Ok(handle) => {
            let resp = CreateThingResponse {
                id: handle.id,
                name: handle.name,
            };
            (StatusCode::CREATED, Json(resp)).into_response()
        }
        Err(e) => {
            tracing::error!(error = %e, campaign_id, "failed to create thing");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to create thing" })),
            )
                .into_response()
        }
    }
}

#[utoipa::path(
    delete,
    path = "/campaign/{id}/things/{thing_id}",
    tag = "things",
    params(
        ("id" = String, Path, description = "Campaign ID"),
        ("thing_id" = String, Path, description = "Thing ID"),
    ),
    responses(
        (status = NO_CONTENT, description = "Thing deleted"),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Not the campaign owner"),
        (status = NOT_FOUND, description = "Campaign or thing not found"),
        (status = 503, description = "Server restarting"),
        (status = 500, description = "Internal error"),
    ),
)]
pub async fn delete_thing(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path((campaign_id, thing_id)): Path<(String, String)>,
) -> impl IntoResponse {
    let supervisor = match resolve_owned_campaign(&state, &user, &campaign_id).await {
        Ok(sup) => sup,
        Err((status, msg)) => {
            return (status, Json(serde_json::json!({ "error": msg }))).into_response();
        }
    };

    let tid = ThingId(Nanoid::from(thing_id));
    match supervisor.ask(DeleteThing { thing_id: tid }).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "Thing not found" })),
        )
            .into_response(),
        Err(e) => {
            tracing::error!(error = %e, campaign_id, "failed to delete thing");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "Failed to delete thing" })),
            )
                .into_response()
        }
    }
}
