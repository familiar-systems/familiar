//! OpenAPI spec for the campaign server.
//!
//! Mirrors `apps/platform/src/openapi.rs`. `api_router()` is the single
//! source of truth for public route registration. The runtime router and
//! the `emit-openapi` binary both consume it, so the spec consumed by TS
//! codegen is always in lockstep with the served routes.
//!
//! Components registered here flow through to
//! `packages/types-campaign/src/openapi/` as imports of the matching ts-rs
//! types (never locally regenerated shapes). ts-rs is the compile-time
//! source of truth for type bodies; utoipa is the source of truth for
//! which routes/methods/params exist.

use crate::routes::catalog::*;
use crate::routes::health::*;
use crate::routes::metadata::*;
use crate::routes::things::*;
use crate::state::AppState;
use familiar_systems_campaign_shared::onboarding::catalog::{
    ByoEntry, CatalogResponse, SystemEntry, TemplateRef,
};
use familiar_systems_campaign_shared::onboarding::initialize::{
    AudioMode, CampaignErrorResponse, PatchCampaignRequest,
};
use familiar_systems_campaign_shared::onboarding::metadata::CampaignMetadataResponse;
use familiar_systems_campaign_shared::things::{CreateThingRequest, CreateThingResponse};
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "familiar.systems campaign API",
        description = "Campaign-scoped endpoints: catalog, metadata, things, and CRDT room bootstrapping.",
        version = "0.1.0",
        license(name = "AGPL-3.0-or-later", identifier = "AGPL-3.0-or-later"),
    ),
    components(schemas(
        CatalogResponse,
        SystemEntry,
        ByoEntry,
        TemplateRef,
        PatchCampaignRequest,
        CampaignErrorResponse,
        AudioMode,
        CampaignMetadataResponse,
        CreateThingRequest,
        CreateThingResponse,
    ))
)]
pub struct ApiDoc;

pub fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .routes(routes!(list_systems))
        .routes(routes!(get_campaign, patch_campaign))
        .routes(routes!(create_thing))
        .routes(routes!(delete_thing))
}
