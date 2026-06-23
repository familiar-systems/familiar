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
use crate::routes::pages::*;
use crate::routes::relationships::*;
use crate::state::AppState;
use familiar_systems_campaign_shared::document::pages::{
    CreateEntityBody, CreatePageRequest, CreateSessionBody, CreateTemplateBody, EntityResponse,
    PageResponse, SessionResponse, TemplateResponse,
};
use familiar_systems_campaign_shared::onboarding::catalog::{
    ByoEntry, CatalogResponse, SystemEntry, TemplateRef,
};
use familiar_systems_campaign_shared::onboarding::initialize::{
    AudioMode, CampaignErrorResponse, PatchCampaignRequest,
};
use familiar_systems_campaign_shared::onboarding::metadata::CampaignMetadataResponse;
use familiar_systems_campaign_shared::relationship::{
    CreateRelationshipRequest, EntitySearchResult, KnowledgeInput, KnowledgeView, OriginInput,
    PatchRelationshipRequest, PredicatePairView, RelatedPage, RelationshipView, SessionRef,
    SessionStampPatch, SessionsResponse, ViewSessionOrdinal, ViewSessionPoint,
};
use familiar_systems_campaign_shared::status::Status;
use utoipa::OpenApi;
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "familiar.systems campaign API",
        description = "Campaign-scoped endpoints: catalog, metadata, and (future) CRDT room bootstrapping.",
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
        CreatePageRequest,
        CreateEntityBody,
        CreateTemplateBody,
        CreateSessionBody,
        PageResponse,
        EntityResponse,
        TemplateResponse,
        SessionResponse,
        Status,
        // Relationships
        RelationshipView,
        RelatedPage,
        ViewSessionPoint,
        ViewSessionOrdinal,
        KnowledgeView,
        CreateRelationshipRequest,
        OriginInput,
        KnowledgeInput,
        PatchRelationshipRequest,
        SessionStampPatch,
        PredicatePairView,
        SessionRef,
        SessionsResponse,
        EntitySearchResult,
    ))
)]
pub struct ApiDoc;

pub fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .routes(routes!(list_systems))
        .routes(routes!(get_campaign, patch_campaign))
        .routes(routes!(create_page))
        .routes(routes!(get_relationships))
        .routes(routes!(create_relationship))
        .routes(routes!(patch_relationship, delete_relationship))
        .routes(routes!(known_predicates))
        .routes(routes!(list_sessions))
        .routes(routes!(search_entities))
}
