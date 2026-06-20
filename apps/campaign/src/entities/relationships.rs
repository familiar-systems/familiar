use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::{
    InvalidationReasonCol, PageIdCol, RelationshipIdCol, SessionIdCol, VisibilityCol,
};

/// One relationship edge: an undirected, page-to-page fact carrying a predicate
/// at each end, plus its temporal provenance.
///
/// Stored canonically (`page_a` is the lexicographically smaller `PageId`, with
/// the predicate pair assigned to match) so each fact has exactly one row and a
/// reversed duplicate is structurally impossible; the owning actor enforces this
/// before any write. Predicates are immutable - evolution creates a new row, it
/// never edits these. Two nullable session FKs encode sum types reconstituted at
/// the `*Col`/domain boundary: `origin_session_id` NULL means `Prior`;
/// `invalidation_reason` is the live/invalidated discriminant (NULL = live), and
/// within an invalidated row `invalidated_by_session_id` carries prior-vs-session
/// (NULL = ended before the campaign began). The actor + petgraph land in Slice
/// 2. See `docs/plans/2026-04-10-entity-relationship-temporal-model.md`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "relationships")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: RelationshipIdCol,
    /// Canonical-smaller endpoint.
    pub page_a: PageIdCol,
    /// Canonical-larger endpoint.
    pub page_b: PageIdCol,
    /// Predicate read from `page_a` toward `page_b` ("is a resident of").
    pub predicate_a_to_b: String,
    /// Predicate read from `page_b` toward `page_a` ("is the home of").
    pub predicate_b_to_a: String,
    pub visibility: VisibilityCol,
    /// `origin = Session(id)`; `None` encodes `Prior` (true before session 1).
    pub origin_session_id: Option<SessionIdCol>,
    /// When the row was written (debug/audit).
    pub created_at: DateTime<Utc>,
    /// The live/invalidated discriminant: `None` = live, `Some` = invalidated.
    pub invalidation_reason: Option<InvalidationReasonCol>,
    /// Within an invalidated row: the session that ended the fact. `None` while
    /// live; also `None` for the (UI-deferred) "ended before the campaign began".
    pub invalidated_by_session_id: Option<SessionIdCol>,
    /// When the row was invalidated (debug/audit). Co-occurs with
    /// `invalidation_reason` (a migration CHECK guards this).
    pub invalidated_at: Option<DateTime<Utc>>,
}

/// Four `belongs_to` edges: both endpoints to `pages` (`Cascade` - an edge can't
/// outlive an endpoint), and the two temporal references to `sessions` (NO
/// ACTION, since their NULLs are load-bearing - `Prior` / ended-in-prior - and
/// must never be silently rewritten). No `Related` impl: the dual `pages` FK
/// makes a single canonical `Related<pages::Entity>` ambiguous, and nothing in
/// this slice traverses via `find_related`.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::pages::Entity",
        from = "Column::PageA",
        to = "super::pages::Column::Id",
        on_delete = "Cascade"
    )]
    PageA,
    #[sea_orm(
        belongs_to = "super::pages::Entity",
        from = "Column::PageB",
        to = "super::pages::Column::Id",
        on_delete = "Cascade"
    )]
    PageB,
    #[sea_orm(
        belongs_to = "super::sessions::Entity",
        from = "Column::OriginSessionId",
        to = "super::sessions::Column::Id"
    )]
    OriginSession,
    #[sea_orm(
        belongs_to = "super::sessions::Entity",
        from = "Column::InvalidatedBySessionId",
        to = "super::sessions::Column::Id"
    )]
    InvalidatedBySession,
}

impl ActiveModelBehavior for ActiveModel {}
