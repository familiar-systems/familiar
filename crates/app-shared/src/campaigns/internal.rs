//! Internal-API wire types for the campaigns subsystem.
//!
//! Rust-only. Both caller and callee are Rust binaries; no TypeScript export
//! is needed. Lives in `app-shared` because both binaries depend on the
//! same shape and a duplicated definition on either side would drift.
//!
//! Each type's docstring names the route owner (the tier whose handler
//! consumes the request) so reviewers can find the handler without
//! grepping both binaries.

use crate::id::{CampaignId, UserId};
use serde::{Deserialize, Serialize};

/// Body for `POST /internal/campaign`.
///
/// **Owner: campaign tier.** Called by the platform tier from inside its
/// `POST /api/campaigns` handler to create a new campaign on the shard.
/// Idempotent on `campaign_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateCampaignRequest {
    pub campaign_id: CampaignId,
    pub owner_user_id: UserId,
}

/// Body for `PATCH /internal/platform/campaign/{id}`.
///
/// **Owner: platform tier.** Called by the campaign tier after any
/// successful metadata update. The platform mirrors these fields onto
/// its `campaigns` row so the hub list can render campaign metadata
/// without a per-shard fan-out. Only changed fields are sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchCampaignMirror {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tagline: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub game_system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wizard_completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Body for `POST /internal/platform/campaign/{id}/init-failed`.
///
/// **Owner: platform tier.** Called by the campaign tier when a
/// wizard-completion attempt fails. The platform persists `reason` onto
/// the matching `campaigns.last_init_error` row so the SPA can render an
/// "initialization failed" badge in the hub list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitFailedRequest {
    pub reason: String,
}

/// Body for `POST /internal/platform/heartbeat`.
///
/// **Owner: platform tier.** Sent periodically by the campaign shard
/// with the list of currently loaded campaign IDs. The platform replaces
/// its in-memory loaded cache wholesale on each heartbeat, reconciling
/// any drift from missed release notifications or restarts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatRequest {
    pub campaigns: Vec<CampaignId>,
}
