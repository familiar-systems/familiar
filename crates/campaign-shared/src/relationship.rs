//! Relationship wire types: the oriented read DTO the widget renders, plus the
//! create/patch request bodies.
//!
//! These are **wire types**, never persisted in a Loro doc - relationships are
//! server-authoritative (the `RelationshipGraph` actor owns them), not CRDT
//! content. So, unlike `status`/`page_kind`, there is no `as_loro_str` contract
//! here; the drift guard lives at the `*Col` boundary in
//! `apps/campaign/src/entities/columns.rs` instead.
//!
//! Scope note: this module holds only the wire surface. The *undirected* in-memory
//! model (`Relationship`, `Origin`, `Knowledge`) the petgraph actor traverses lives
//! app-local in `apps/campaign/src/domain/relationship.rs` (pure server-internal
//! algebra, no TS surface - the client only ever sees the oriented view, working in
//! session ordinals, never raw `SessionId`s).
//!
//! A relationship moves along two orthogonal, authored, session-stamped axes:
//! **factuality** `[origin, superseded)` plus a terminal retcon, and **knowledge**
//! (`Public | Hidden | Revealed(s)`). See
//! `docs/plans/2026-06-23-entity-relationship-temporal-model.md`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::{PageId, RelationshipId, SessionId};

/// The other endpoint of a relationship, as the viewer of one page sees it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct RelatedPage {
    pub id: PageId,
    pub name: String,
}

/// A point on the factuality origin axis, in the viewer's terms: before the campaign
/// began, or at a session (by its curated ordinal). A sum rather than a nullable
/// ordinal so `Prior` is a first-class value the client can't confuse with a missing
/// field. (Only `origin` can be `Prior`; the session-only axes -
/// superseded/retcon/reveal - use a bare [`ViewSessionOrdinal`].) Adjacent tagging
/// (`{ "kind": "...", "content": { ... } }`) per the convention guard in
/// `crates/app-shared/tests/conventions.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum ViewSessionPoint {
    /// Before the campaign's first session.
    Prior,
    /// At a session, by its curated ordinal.
    Session(ViewSessionOrdinal),
}

/// A session referred to by its curated ordinal, in the viewer's terms. Used bare
/// for the session-only axes (superseded / retcon / reveal) and inside
/// [`ViewSessionPoint::Session`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct ViewSessionOrdinal {
    /// "Session N". Serialized as a JSON number (counts are small); `#[ts(type)]`
    /// keeps the TS type `number` rather than ts-rs's default `bigint` for `i64`.
    #[ts(type = "number")]
    pub ordinal: i64,
}

/// The knowledge axis in the viewer's terms: public (always known), secret and not yet
/// revealed, or revealed to the players at a session. Mirrors the server-internal
/// `Knowledge` sum; the secret bit is implicit in the variant (`Public` = not secret,
/// the others secret). Adjacent tagging per the convention guard.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum KnowledgeView {
    /// Born public: known to the players from the moment it became true.
    Public,
    /// Born secret, not yet revealed (GM-only). Drives the row's GM-wash.
    Hidden,
    /// Born secret, learned by the players at this session.
    Revealed(ViewSessionOrdinal),
}

/// One relationship as rendered on a given page: oriented so the client never
/// computes direction, and projected onto both axes. The server picks `predicate`
/// (forward *from the viewed page*) and `predicate_reverse` (back toward it) from
/// the stored undirected pair, and resolves session identities to ordinals.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct RelationshipView {
    pub id: RelationshipId,
    /// The page at the other end of the edge.
    pub other: RelatedPage,
    /// The predicate read forward, from the viewed page toward `other`.
    pub predicate: String,
    /// The predicate read back, from `other` toward the viewed page.
    pub predicate_reverse: String,
    /// Factuality start.
    pub origin: ViewSessionPoint,
    /// Factuality end: the session the fact stopped being true. `None` = still true.
    pub superseded: Option<ViewSessionOrdinal>,
    /// The session a retcon struck the fact. `None` = not retconned.
    pub retcon: Option<ViewSessionOrdinal>,
    /// Knowledge axis.
    pub knowledge: KnowledgeView,
}

// ---------------------------------------------------------------------------
// Request bodies
// ---------------------------------------------------------------------------

/// Where a relationship became true, as a create supplies it: the input analog of
/// the output [`ViewSessionPoint`]. A sum, not a nullable `SessionId`, so `Prior`
/// is a first-class choice the picker offers. The client sends back a `SessionId`
/// it got from `GET /sessions`, so a raw `SessionId` crossing the wire as *input*
/// is fine (the no-raw-`SessionId` rule is about the read view's ordinals).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum OriginInput {
    Prior,
    Session(SessionId),
}

/// A knowledge state as the client supplies it - on create (the born state) and on
/// patch (the new state, set wholesale). The input analog of [`KnowledgeView`]: public,
/// secret (hidden), or secret and revealed at a session.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum KnowledgeInput {
    Public,
    Hidden,
    Revealed(SessionId),
}

/// `POST /campaign/{id}/relationships` body. Creating a relationship names both
/// endpoints (it is undirected, owned by neither page); `subject_page_id` /
/// `other_page_id` set the orientation and the server canonicalizes by id.
/// `supersedes` makes it an atomic replace: the named live relationship is ended in
/// the same transaction at this create's origin session (so `origin` must be a
/// session, not `Prior`, when `supersedes` is set).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct CreateRelationshipRequest {
    pub subject_page_id: PageId,
    pub other_page_id: PageId,
    /// Reads subject -> other.
    pub predicate_forward: String,
    /// Reads other -> subject.
    pub predicate_reverse: String,
    pub origin: OriginInput,
    pub knowledge: KnowledgeInput,
    /// The live relationship this one replaces, ended atomically. `None` for a
    /// plain create.
    pub supersedes: Option<RelationshipId>,
}

/// A patch to one nullable session-stamp axis: set it to a session, or clear it back
/// to NULL (the reversible correction). A field left absent (`null`) on
/// [`PatchRelationshipRequest`] leaves that axis unchanged; present-and-`Clear` is an
/// explicit un-set. Adjacent tagging per the convention guard.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum SessionStampPatch {
    /// Stamp the axis with this session.
    Set(SessionId),
    /// Clear the axis back to NULL.
    Clear,
}

/// `PATCH /campaign/{id}/relationships/{relId}` body: independent, reversible edits to
/// the three mutable axes. Each field optional (absent = leave that axis unchanged); at
/// least one must be present. Predicates and origin are immutable, so they are absent
/// here. The present edits apply as one atomic batch.
///
/// `knowledge` is set wholesale to the new state (`Public | Hidden | Revealed(s)`),
/// freely - reveal, conceal, or re-hide. `superseded` is the factuality end (set = end,
/// clear = un-end); `retcon` is the terminal strike (set = retcon, clear = un-retcon).
/// End-*with-successor* is not a patch - it goes through `POST /relationships` with
/// `supersedes` (the successor is a new row).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct PatchRelationshipRequest {
    pub knowledge: Option<KnowledgeInput>,
    pub superseded: Option<SessionStampPatch>,
    pub retcon: Option<SessionStampPatch>,
}

// ---------------------------------------------------------------------------
// Auxiliary read responses
// ---------------------------------------------------------------------------

/// One known predicate pair and its usage count, for the create modal's predicate
/// typeahead + reverse autofill. Wire form of the server-internal `PredicatePair`.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct PredicatePairView {
    pub forward: String,
    pub reverse: String,
    pub count: u32,
}

/// A session by its durable id + curated ordinal, for the as-of pickers. The id is
/// what create/patch send back (ordinals can be renumbered; ids are stable).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct SessionRef {
    pub id: SessionId,
    #[ts(type = "number")]
    pub ordinal: i64,
}

/// `GET /campaign/{id}/sessions`: every session ascending by ordinal, plus the
/// current (max-ordinal) one for the picker's default. `current` is `None` when the
/// campaign has no sessions yet (the picker then offers `Prior` only).
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct SessionsResponse {
    pub sessions: Vec<SessionRef>,
    pub current: Option<SessionRef>,
}

/// A matched entity page from `GET /campaign/{id}/entities?q=`, for the object
/// typeahead.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct EntitySearchResult {
    pub id: PageId,
    pub name: String,
}
