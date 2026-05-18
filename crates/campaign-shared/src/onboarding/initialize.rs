//! `POST /campaign/<id>/initialize` request shape.
//!
//! v0 thin slice: the campaign tier validates the payload, fires a callback
//! to the platform's `init-failed` endpoint, and returns 500. The shape
//! exists in its final form so the next slice can replace the failure path
//! with the real init transaction without breaking the FE.

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
pub struct InitializeRequest {
    /// Opaque display label for the campaign's game system. The wizard
    /// resolves a single string at Seal time: catalog pick -> the catalog
    /// entry's locale-resolved `name`; BYO with a typed name -> that name;
    /// BYO with an empty input -> the catalog's `byo.default_name`. The
    /// campaign tier and platform tier both treat this as an opaque label;
    /// no special slugs cross the wire.
    pub game_system: String,
    /// BCP-47 language tag for the campaign's content (sticky once set).
    pub content_locale: String,
    pub name: String,
    pub tagline: Option<String>,
    /// Bundle slugs the GM picked. Deduped client-side.
    pub template_slugs: Vec<String>,
    pub audio: AudioMode,
    pub evals_enabled: bool,
}

/// Returned on the deliberate failure path of v0. The FE renders `error`
/// inline and surfaces `campaign_id` so the user can find their (broken)
/// campaign back in the hub.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct InitializeErrorResponse {
    pub error: String,
    pub campaign_id: String,
}
