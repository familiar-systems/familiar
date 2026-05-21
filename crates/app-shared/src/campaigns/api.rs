//! Public-API wire types for the campaigns subsystem.
//!
//! **Wire format we own.** Shapes emitted to / accepted from the SPA and
//! exported to TypeScript via ts-rs. Changes here are breaking changes to
//! consumers.

use crate::id::CampaignId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

/// Body for `POST /api/campaigns`.
///
/// The SPA mints `idempotency_token` (a fresh nanoid per click) so that a
/// retry of the same logical create call returns the same `CampaignId`
/// rather than allocating a new one.
#[derive(Debug, Clone, Deserialize, Serialize, TS, ToSchema)]
#[ts(export, export_to = "types-app/src/generated/campaigns/")]
pub struct CreateCampaignRequest {
    pub idempotency_token: String,
}

/// Response body for `POST /api/campaigns`.
#[derive(Debug, Clone, Serialize, TS, ToSchema)]
#[ts(export, export_to = "types-app/src/generated/campaigns/")]
pub struct CreateCampaignResponse {
    pub campaign_id: CampaignId,
}

/// One row in the response of `GET /api/campaigns`.
///
/// Mirrored from the campaign tier on every successful initialize and
/// settings edit. Fields marked `Option` are `None` until the wizard's
/// Seal call commits, after which they are mirrored over.
///
/// `last_init_error`: populated by the campaign tier via
/// `POST /internal/platform/campaigns/<id>/init-failed` when an initialize
/// attempt fails. Distinct from "no init attempt yet" (`wizard_completed_at IS NULL`
/// AND `last_init_error IS NULL`).
#[derive(Debug, Clone, Serialize, TS, ToSchema)]
#[ts(export, export_to = "types-app/src/generated/campaigns/")]
pub struct Campaign {
    pub id: CampaignId,
    pub name: Option<String>,
    pub tagline: Option<String>,
    pub game_system: Option<String>,
    pub content_locale: Option<String>,
    pub last_init_error: Option<String>,
    pub loaded: bool,
    #[ts(type = "string | null")]
    pub wizard_completed_at: Option<DateTime<Utc>>,
    #[ts(type = "string")]
    pub created_at: DateTime<Utc>,
    #[ts(type = "string")]
    pub updated_at: DateTime<Utc>,
}
