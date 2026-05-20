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

/// Body for `POST /internal/campaign/init`.
///
/// **Owner: campaign tier.** Called by the platform tier from inside its
/// `POST /api/campaigns` handler. Idempotent; a repeat call with the same
/// `campaign_id` no-ops on the campaign side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InternalInitRequest {
    pub campaign_id: CampaignId,
    pub owner_user_id: UserId,
}

/// Body for `POST /internal/platform/campaigns/<id>/metadata`.
///
/// **Owner: platform tier.** Called by the campaign tier after a
/// successful wizard seal. The platform mirrors these fields onto its
/// `campaigns` row so the hub list can render campaign metadata without
/// a per-shard fan-out.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataMirrorRequest {
    pub name: String,
    pub tagline: Option<String>,
    pub game_system: String,
    pub content_locale: String,
    pub wizard_completed_at: chrono::DateTime<chrono::Utc>,
}

/// Body for `POST /internal/platform/campaigns/<id>/init-failed`.
///
/// **Owner: platform tier.** Called by the campaign tier when an
/// initialize attempt fails. The platform persists `reason` onto the
/// matching `campaigns.last_init_error` row so the SPA can render an
/// "initialization failed" badge in the hub list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitFailedRequest {
    pub reason: String,
}
