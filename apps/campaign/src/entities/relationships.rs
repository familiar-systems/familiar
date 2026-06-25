use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::{PageIdCol, RelationshipIdCol, SessionIdCol};

/// One relationship edge: an undirected, page-to-page fact carrying a predicate
/// at each end, plus its two temporal axes.
///
/// Stored canonically (`page_a` is the lexicographically smaller `PageId`, with
/// the predicate pair assigned to match) so each fact has exactly one row and a
/// reversed duplicate is structurally impossible; the owning actor enforces this
/// before any write. Predicates are immutable - evolution creates a new row, it
/// never edits these.
///
/// Four nullable session FKs encode sum types reconstituted at the `*Col`/domain
/// boundary. **Factuality** `[origin, superseded)`: `origin_session_id` NULL =
/// `Prior`; `superseded_session_id` NULL = still true (and a non-NULL value is the
/// live/ended discriminant - see the partial unique index); `retcon_session_id`
/// NULL = not retconned. **Knowledge** (freely mutable): `is_secret` (NOT NULL) plus
/// `reveal_session_id` reconstitute `Public | Hidden | Revealed(s)` - a public fact
/// never carries a reveal (a CHECK guards it). See
/// `docs/plans/2026-06-23-entity-relationship-temporal-model.md`.
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
    /// `origin = Session(id)`; `None` encodes `Prior` (true before session 1).
    pub origin_session_id: Option<SessionIdCol>,
    /// The session the fact stopped being true. `None` = still true; this column
    /// is also the live/ended discriminant (ending is always at a session).
    pub superseded_session_id: Option<SessionIdCol>,
    /// The session a retcon struck the fact from the fiction. `None` = not
    /// retconned.
    pub retcon_session_id: Option<SessionIdCol>,
    /// `false` = public (always known to the players); `true` = secret (`Hidden` or
    /// `Revealed`). Freely mutable.
    pub is_secret: bool,
    /// The session the players learned a secret fact. `None` = not revealed. A CHECK
    /// forbids a non-NULL value on a public row.
    pub reveal_session_id: Option<SessionIdCol>,
    /// When the row was written (debug/audit).
    pub created_at: DateTime<Utc>,
}

/// Six `belongs_to` edges: both endpoints to `pages` (`Cascade` - an edge can't
/// outlive an endpoint), and the four temporal references to `sessions` (NO
/// ACTION, since their NULLs are load-bearing - `Prior` / still-true / not-retconned
/// / not-revealed - and must never be silently rewritten). No `Related` impl: the
/// dual `pages` FK makes a single canonical `Related<pages::Entity>` ambiguous, and
/// nothing in this slice traverses via `find_related`.
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
        from = "Column::SupersededSessionId",
        to = "super::sessions::Column::Id"
    )]
    SupersededSession,
    #[sea_orm(
        belongs_to = "super::sessions::Entity",
        from = "Column::RetconSessionId",
        to = "super::sessions::Column::Id"
    )]
    RetconSession,
    #[sea_orm(
        belongs_to = "super::sessions::Entity",
        from = "Column::RevealSessionId",
        to = "super::sessions::Column::Id"
    )]
    RevealSession,
}

impl ActiveModelBehavior for ActiveModel {}
