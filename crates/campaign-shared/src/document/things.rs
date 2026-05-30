//! `POST /campaign/{id}/things` request and response shapes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::ThingId;
use crate::status::Status;

/// Create a new Thing in a campaign.
///
/// `name` is required; everything else is optional. A missing `status` defaults
/// to `gmOnly` (the domain default for new content). `parent` places the Thing
/// in the table of contents: omitted appends it at the ToC root; `Some(id)`
/// nests it as the last child of that Thing's ToC node. `from_template_id` is
/// accepted but not yet implemented (the server returns 501).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct CreateThingRequest {
    pub name: String,
    pub status: Option<Status>,
    /// Parent Thing to nest under in the table of contents. `None` => ToC root.
    pub parent: Option<ThingId>,
    /// Template to clone from. Not yet supported; a value here yields 501.
    pub from_template_id: Option<ThingId>,
}

/// A created Thing, returned with `201 Created`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct ThingResponse {
    pub id: ThingId,
    pub name: String,
    pub status: Status,
    pub prototype_id: Option<ThingId>,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp.
    pub updated_at: String,
}
