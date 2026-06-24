//! Re-exports the shared [`AuthenticatedUser`] extractor and wires it to
//! this binary's [`AppState`] via `FromRef`. Also provides [`authorize_gm`], the
//! campaign-local authorization step every GM-only campaign route shares.

use std::sync::Arc;

use axum::extract::FromRef;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use familiar_systems_app_shared::auth::HankoSessionValidator;
use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_app_shared::middleware::internal_auth::InternalBearerConfig;
use fs_id::Nanoid;

use crate::actors::registry::{CampaignHandle, GetCampaign};
use crate::state::AppState;

pub use familiar_systems_app_shared::auth::AuthenticatedUser;

impl FromRef<AppState> for Arc<HankoSessionValidator> {
    fn from_ref(state: &AppState) -> Self {
        state.validator.clone()
    }
}

impl FromRef<AppState> for InternalBearerConfig {
    fn from_ref(state: &AppState) -> Self {
        InternalBearerConfig {
            primary: state.config.internal_bearer_primary.clone(),
            secondary: state.config.internal_bearer_secondary.clone(),
        }
    }
}

/// Resolve a GM-only campaign route: look the campaign up in the registry, then
/// confirm the (already-authenticated) caller's `Gm` membership on the platform tier.
/// Returns the branded `CampaignId` and the campaign's [`CampaignHandle`] (the
/// supervisor for picker reads, plus the `RelationshipGraph` ref the graph routes talk
/// to directly) on success; on failure returns a ready-to-send response with the exact
/// status each route used before - `404` (campaign not on this shard), `403` (not a
/// GM), `503` (registry or platform unreachable).
///
/// This is the duplicated half of every GM route's preamble, pulled out once.
/// Authentication stays an [`AuthenticatedUser`] extractor on the handler (so
/// `401` precedes this call), and any body-shaped precondition a route enforces
/// before authorization - e.g. `create_page`'s `501` for an unbuilt template -
/// stays in the handler ahead of this call, preserving each route's error order.
///
/// GM membership is the cross-user boundary, so it is checked server-side here,
/// never trusted from the client (see the threat model in the root CLAUDE.md).
pub async fn authorize_gm(
    state: &AppState,
    campaign_id: String,
    user: &AuthenticatedUser,
) -> Result<(CampaignId, CampaignHandle), Response> {
    let cid = CampaignId::from(Nanoid::from(campaign_id));

    let handle = match state.registry.ask(GetCampaign(cid.clone())).await {
        Ok(Some(handle)) => handle,
        Ok(None) => return Err(StatusCode::NOT_FOUND.into_response()),
        Err(_) => return Err(StatusCode::SERVICE_UNAVAILABLE.into_response()),
    };

    let caller = UserId(user.id);
    match state
        .platform_internal
        .check_membership(&cid.0.0, &caller)
        .await
    {
        Ok(Some(CampaignRole::Gm)) => {}
        Ok(Some(_)) | Ok(None) => return Err(StatusCode::FORBIDDEN.into_response()),
        Err(e) => {
            tracing::warn!(error = %e, "membership check failed during GM authorization");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    }

    Ok((cid, handle))
}
