use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

/// Visibility status for campaign content. The CRDT syncs all content to all
/// clients regardless of status; consumers (the browser UI, AI conversations)
/// filter what they surface based on the user's role.
/// See: docs/plans/2026-02-22-ai-prd.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, ToSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum Status {
    /// This is known only to the GM.
    /// It could be a secret plot point or hidden story arc.
    /// Or it could be some piece of lore or background that the GM hasn't decided on yet.
    /// Regardless, only the GM is aware of it but AI treats it as fact.
    GmOnly,
    /// This is known to players.
    /// It has either been revealed through play or the GM has explicitly shared it.
    Known,
    /// This was canon but has been retconned during play.
    Retconned,
}
