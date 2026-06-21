//! Relationship wire types: the enums both servers' at-rest boundary and the
//! widget share, plus the oriented read DTO the widget renders.
//!
//! These are **wire types**, never persisted in a Loro doc - relationships are
//! server-authoritative (the `RelationshipGraph` actor owns them), not CRDT
//! content. So, unlike `status`/`page_kind`, there is no `as_loro_str` contract
//! here; the drift guard lives at the `*Col` boundary in
//! `apps/campaign/src/entities/columns.rs` instead.
//!
//! Scope note: this module holds only the wire read surface - `Visibility`,
//! `InvalidationReason`, and `RelationshipView`. The *undirected* in-memory model
//! (`Relationship`, `Origin { Prior, Session(SessionId) }`, `Invalidation`) the
//! petgraph actor traverses lives app-local in
//! `apps/campaign/src/domain/relationship.rs` (pure server-internal algebra, no TS
//! surface - the client only ever sees the oriented view, working in session
//! ordinals, never raw `SessionId`s).
//!
//! See `docs/plans/2026-04-10-entity-relationship-temporal-model.md`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::id::{PageId, RelationshipId};

/// Who may see a relationship. Mutable and independent of `origin`: the GM can
/// reveal or hide a fact at any time without invalidating it. Two values for
/// now; per-player visibility is a future expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, ToSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum Visibility {
    Gm,
    Players,
}

/// Why a relationship row stopped being live. The *presence* of a reason is the
/// at-rest live/invalidated discriminant (a live row has none). `Superseded`
/// covers both narrative end and replacement (it stays visible in historical
/// snapshots); `Retconned` means "never true in the fiction" (excluded from
/// snapshots, kept in the database as part of the tapestry).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, ToSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum InvalidationReason {
    Superseded,
    Retconned,
}

/// The other endpoint of a relationship, as the viewer of one page sees it.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct RelatedPage {
    pub id: PageId,
    pub name: String,
}

/// A point in knowledge time, in the viewer's terms: before the campaign began, or
/// at a session (by its curated ordinal). A sum rather than a nullable ordinal so
/// `Prior` is a first-class value the client can't confuse with a missing field.
/// Reused by both a relationship's `origin` and a superseded end, mirroring the
/// server-internal `Origin` sum that backs both. Adjacent tagging
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

/// The session a `ViewSessionPoint::Session` refers to, by its curated ordinal.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, ToSchema)]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub struct ViewSessionOrdinal {
    /// "Session N". Serialized as a JSON number (counts are small); `#[ts(type)]`
    /// keeps the TS type `number` rather than ts-rs's default `bigint` for `i64`.
    #[ts(type = "number")]
    pub ordinal: i64,
}

/// How a no-longer-live relationship was invalidated, in the viewer's terms. The
/// reason is the discriminant, each variant carrying only what it renders:
/// `Superseded` (narrative end or replacement) carries when it ended - a session
/// point, possibly `Prior` for the rare ended-before-the-campaign case; `Retconned`
/// ("never true in the fiction") carries nothing and renders off the `origin`.
/// Adjacent tagging per the convention guard.
#[derive(Debug, Clone, Serialize, Deserialize, TS, ToSchema)]
#[serde(tag = "kind", content = "content", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/relationship/")]
pub enum ViewInvalidation {
    Superseded { ended: ViewSessionPoint },
    Retconned,
}

/// One relationship as rendered on a given page: oriented so the client never
/// computes direction. The server picks `predicate` (forward *from the viewed
/// page*) and `predicate_reverse` (back toward it) from the stored undirected
/// pair, and resolves session identities to ordinals.
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
    pub visibility: Visibility,
    pub origin: ViewSessionPoint,
    /// `None` for a live relationship.
    pub invalidation: Option<ViewInvalidation>,
}
