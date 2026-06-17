//! `POST /campaign/{id}/pages` request and response shapes.
//!
//! Page creation is one endpoint over a kind-tagged union: both the request and
//! the response are **adjacent-tagged** discriminated unions over `PageKind`
//! (`{ "kind": "...", "content": { ... } }`). Each kind carries only the fields
//! it actually has - a template has no `from_template_id`, a session's name is
//! optional and its response carries `ordinal`/`session_id`. Adding a page kind
//! is adding a variant the compiler points at, end to end (Rust + TS).
//!
//! Adjacent (not internal) tagging is deliberate: utoipa 5.x emits a clean
//! `oneOf` + discriminator for `#[serde(tag, content)]` that survives codegen,
//! whereas a bare `#[serde(tag)]` degrades to a stringly-typed union. The
//! `crates/app-shared/tests/conventions.rs` guard enforces this for every
//! `ToSchema` enum.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::{PageId, SessionId};
use crate::status::Status;

/// Create a new Page in a campaign, discriminated by `kind`. The wire shape is
/// `{ "kind": "entity" | "template" | "session", "content": { ... } }`.
///
/// A missing `status` defaults to `gmOnly` (the domain default for new content).
/// `parent` places the Page in the table of contents: omitted appends it at the
/// ToC root; `Some(id)` nests it as the last child of that Page's ToC node.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum CreatePageRequest {
    Entity(CreateEntityBody),
    Template(CreateTemplateBody),
    Session(CreateSessionBody),
}

/// Body for creating an `entity` page (authored world content - an NPC, a
/// location, a bit of lore). `name` is required.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct CreateEntityBody {
    pub name: String,
    pub status: Option<Status>,
    /// Parent Page to nest under in the table of contents. `None` => ToC root.
    /// FIXME this should probably the ID of the ToC block and not a `PageId`.
    /// This would let us nest pages under, say, a Folder.
    pub parent: Option<PageId>,
    /// Template to clone from. Not yet supported; a value here yields 501.
    pub from_template_id: Option<PageId>,
}

/// Body for creating a `template` page - the prototype other entities clone
/// from. A template never clones from another template, so it carries no
/// `from_template_id`. `name` is required.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct CreateTemplateBody {
    pub name: String,
    pub status: Option<Status>,
    /// Parent Page to nest under in the table of contents. `None` => ToC root.
    pub parent: Option<PageId>,
}

/// Body for creating a `session` page - its document plus its temporal record,
/// minted together in one genesis transaction.
///
/// `name` is the GM's optional subtitle ("The End of Perth"); omitted or blank
/// means an unnamed session, identified by its ordinal until the GM titles it
/// after play.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct CreateSessionBody {
    pub name: Option<String>,
    pub status: Option<Status>,
    /// Parent to nest under in the table of contents. `None` => ToC root.
    pub parent: Option<PageId>,
}

/// A created Page, returned with `201 Created`, discriminated by `kind`. The
/// wire shape mirrors the request: `{ "kind": "...", "content": { ... } }`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum PageResponse {
    Entity(EntityResponse),
    Template(TemplateResponse),
    Session(SessionResponse),
}

/// A created `entity` page.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct EntityResponse {
    pub id: PageId,
    pub name: String,
    pub status: Status,
    /// Lineage: the template this entity was cloned from, if any.
    pub template_id: Option<PageId>,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp.
    pub updated_at: String,
}

/// A created `template` page. A template has no `template_id` lineage of its own
/// (it is the source, not a clone).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct TemplateResponse {
    pub id: PageId,
    pub name: String,
    pub status: Status,
    /// RFC 3339 timestamp.
    pub created_at: String,
    /// RFC 3339 timestamp.
    pub updated_at: String,
}

/// A created `session`: its page plus its temporal record.
///
/// The display name is `Session {ordinal}` (plus `: {name}` when the GM named
/// it); the client composes it from `ordinal` and the page title (`name`).
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
