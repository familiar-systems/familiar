//! `PATCH /campaign/{id}` request shape.
//!
//! All metadata fields are optional. When `wizard_complete` is true the
//! server validates that all required fields are present (either in this
//! request or already persisted), sets `wizard_completed_at`, and mirrors
//! to the platform. Without the flag, only the provided fields are updated.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

/// Audio capture / training mode chosen during the wizard.
///
/// Three states modeled as a sum type, not a `bool` pair, so the FE
/// can't accidentally produce "no recording but train on it" or other
/// nonsense combinations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
#[serde(rename_all = "kebab-case")]
pub enum AudioMode {
    /// Record sessions; opt in to having that audio improve transcription.
    OptIn,
    /// Record sessions; audio is processed for transcription only and
    /// excluded from any training run.
    OptOut,
    /// Never record audio. The GM pastes notes by hand.
    TextOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct PatchCampaignRequest {
    /// Opaque display label for the campaign's game system. The wizard
    /// resolves a single string at submission: catalog pick uses the
    /// locale-resolved `name`; BYO uses the typed name or the catalog's
    /// `byo.default_name`. Both tiers treat this as an opaque label.
    pub game_system: Option<String>,
    /// BCP-47 language tag for the campaign's content (sticky once set).
    pub content_locale: Option<String>,
    pub name: Option<String>,
    pub tagline: Option<String>,
    /// Bundle slugs the GM picked. Deduped client-side.
    pub template_slugs: Option<Vec<String>>,
    pub audio: Option<AudioMode>,
    pub evals_enabled: Option<bool>,
    /// When true, validates all required fields are present and sets
    /// `wizard_completed_at`. Returns 409 if the wizard was already
    /// completed. Subsequent PATCHes without this flag update metadata
    /// without touching the wizard timestamp.
    #[serde(default)]
    pub wizard_complete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct CampaignErrorResponse {
    pub error: String,
    pub campaign_id: String,
}
