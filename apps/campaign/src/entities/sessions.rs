use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::{PageIdCol, SessionIdCol};

/// A session: the campaign's atomic unit of knowledge time.
///
/// The temporal record (the durable identity every relationship's `origin` /
/// `invalidated_by` points at, plus the GM-curated sequence number used by
/// snapshot/diff queries), now linked to its Session page. Born together with
/// that page in one transaction by the supervisor's `CreateSession` workflow;
/// `ordinal` is auto-assigned there as `max + 1`. The reorder operation and the
/// inbound relationship edges land in later slices.
///
/// This row is **purely temporal**: a session's human label is its page title
/// (`pages.name`), not stored here. Its identity is the `ordinal`, so the label
/// is an optional convenience, not a unique key. See
/// `docs/plans/2026-04-10-entity-relationship-temporal-model.md`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "sessions")]
pub struct Model {
    /// Immutable identity. Relationship `origin` / `invalidated_by` reference
    /// this, never the mutable `ordinal`, so a reorder never disturbs provenance.
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: SessionIdCol,
    /// GM-curated "Session N". Mutable: a reorder reflows historical snapshots to
    /// the GM's current canon. The snapshot/diff queries compare against it.
    /// Unique within the campaign (mirrored by the migration's `unique_key`).
    #[sea_orm(unique)]
    pub ordinal: i64,
    /// When the row was written. Immutable; the recording-order axis, kept
    /// distinct from the curated `ordinal`.
    pub created_at: DateTime<Utc>,
    /// Last mutation. Equals `created_at` at genesis; the future reorder
    /// operation bumps it when it reflows `ordinal`. Distinct from the immutable
    /// `created_at` so "renumbered when" is recoverable (mirrors `pages`/`blocks`).
    pub updated_at: DateTime<Utc>,
    /// The Session page that documents this session (sessions-as-pages). Set in
    /// the genesis txn; nullable because the temporal record is the durable half
    /// and may outlive its page (`ON DELETE SET NULL`). Unique: one session per
    /// page (mirrors the migration's inline `unique_key`; many NULLs allowed).
    #[sea_orm(unique)]
    pub page_id: Option<PageIdCol>,
}

/// `belongs_to` the page it documents. `SetNull` matches the migration's FK so
/// the temporal row survives a page deletion (provenance is durable).
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::pages::Entity",
        from = "Column::PageId",
        to = "super::pages::Column::Id",
        on_delete = "SetNull"
    )]
    Page,
}

impl Related<super::pages::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Page.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
