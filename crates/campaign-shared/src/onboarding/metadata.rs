//! `GET /campaign/<id>` response shape.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct CampaignMetadataResponse {
    pub campaign_id: String,
    pub name: String,
    pub tagline: Option<String>,
    pub game_system: Option<String>,
    pub content_locale: Option<String>,
    /// ISO 8601 timestamp, present once the wizard has been completed.
    pub wizard_completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
