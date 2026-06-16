//! `POST /campaign/{id}/sessions` request and response shapes.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::{PageId, SessionId};
use crate::status::Status;

/// Create a new session in a campaign: its Session page and its temporal record,
/// minted together in one genesis transaction.
///
/// Everything is optional. `name` is the GM's subtitle ("The End of Perth");
/// omitted or blank means an unnamed session, identified by its ordinal until
/// the GM titles it after play. A missing `status` defaults to `gmOnly`.
/// `parent` places the page in the table of contents (omitted => ToC root).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct CreateSessionRequest {
    pub name: Option<String>,
    pub status: Option<Status>,
    /// Parent to nest under in the table of contents. `None` => ToC root.
    pub parent: Option<PageId>,
}

/// A created session, returned with `201 Created`.
///
/// The display name is `Session {ordinal}` (plus `: {name}` when the GM named
/// it); the client composes it, since the sequence number is the `ordinal` and
/// the label is the page title.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct SessionResponse {
    /// The Session page (its document, at `/p/{page_id}`).
    pub page_id: PageId,
    /// The temporal record - the durable identity relationships reference.
    pub session_id: SessionId,
    /// The GM-curated sequence number ("Session N"). Serialized as a JSON number
    /// (session counts are small); `#[ts(type)]` keeps the TS type `number`
    /// rather than ts-rs's default `bigint` for `i64`.
    #[ts(type = "number")]
    pub ordinal: i64,
    /// The session's label: its page title (a neutral default when unnamed).
    pub name: String,
    /// RFC 3339 timestamp (the session's recording time).
    pub created_at: String,
}
