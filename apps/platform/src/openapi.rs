//! OpenAPI spec for the platform server.
//!
//! `api_router()` is the single source of truth for route registration. Both
//! the runtime axum router (`crate::routes::router`) and the
//! `emit-openapi` binary call it, which is how we guarantee the served spec
//! at `/openapi.json` and the spec consumed by TS codegen are byte-identical.
//!
//! Components registered here flow through to `packages/types-app/src/openapi/`
//! as imports of the matching ts-rs types — never as locally regenerated
//! shapes. ts-rs is the compile-time source of truth for type bodies; utoipa
//! is the source of truth for which routes/methods/params exist.

use crate::routes::health::*;
use crate::routes::me::*;
use crate::state::AppState;
use familiar_systems_app_shared::auth::MeResponse;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use utoipa::{Modify, OpenApi};
use utoipa_axum::{router::OpenApiRouter, routes};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "familiar.systems platform API",
        description = "Auth, campaign CRUD, routing table, and shard coordination.",
        version = "0.1.0",
        license(name = "AGPL-3.0-or-later", identifier = "AGPL-3.0-or-later"),
    ),
    components(schemas(MeResponse, UserId, CampaignId)),
    modifiers(&BearerAuth),
)]
pub struct ApiDoc;

/// Declares the `bearerAuth` security scheme so handlers can attach
/// `security(("bearerAuth" = []))`. Hanko issues opaque session tokens,
/// not JWTs, but the HTTP Bearer scheme is the right OpenAPI representation.
struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "bearerAuth",
                SecurityScheme::Http(
                    HttpBuilder::new()
                        .scheme(HttpAuthScheme::Bearer)
                        .bearer_format("Hanko session token")
                        .build(),
                ),
            );
        }
    }
}

/// Builds the `OpenApiRouter` carrying every platform route.
///
/// This is the only place handlers are registered. The runtime router and
/// the `emit-openapi` binary both consume what comes out of here.
pub fn api_router() -> OpenApiRouter<AppState> {
    OpenApiRouter::with_openapi(ApiDoc::openapi())
        .routes(routes!(health))
        .routes(routes!(me))
}
