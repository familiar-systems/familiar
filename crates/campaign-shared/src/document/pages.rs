//! `POST /campaign/{id}/pages` request and response shapes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::PageId;
use crate::page_kind::PageKind;
use crate::status::Status;

/// Create a new Page in a campaign.
///
/// `name` is required; everything else is optional. A missing `status` defaults
/// to `gmOnly` (the domain default for new content). `parent` places the Page
/// in the table of contents: omitted appends it at the ToC root; `Some(id)`
/// nests it as the last child of that Page's ToC node. `from_template_id` is
/// accepted but not yet implemented (the server returns 501).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct CreatePageRequest {
    pub name: String,
    pub status: Option<Status>,
    /// Parent Page to nest under in the table of contents. `None` => ToC root.
    /// FIXME this should probably the ID of the ToC block and not a `PageId`.
    /// This would let us nest pages under, say, a Folder.
    pub parent: Option<PageId>,
    /// Template to clone from. Not yet supported; a value here yields 501.
    pub from_template_id: Option<PageId>,
}

/// A created Page, returned with `201 Created`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct PageResponse {
    pub id: PageId,
    pub name: String,
    pub status: Status,
    pub kind: PageKind,
    pub prototype_id: Option<PageId>,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp.
    pub updated_at: String,
}
