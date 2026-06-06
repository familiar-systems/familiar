//! `GET /campaign/<id>` response shape.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::PageId;

#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct CampaignMetadataResponse {
    pub campaign_id: String,
    pub name: String,
    pub tagline: Option<String>,
    pub game_system: Option<String>,
    pub content_locale: Option<String>,
    /// The campaign's home / landing-page Page ("Campaign Base Camp"), seeded
    /// at creation. The client opens this as the campaign's home base. Absent
    /// only in the brief window before the async seed completes.
    pub home_page_id: Option<PageId>,
    /// ISO 8601 timestamp, present once the wizard has been completed.
    pub wizard_completed_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
