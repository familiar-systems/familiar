use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::SessionIdCol;

/// A session: the campaign's atomic unit of knowledge time.
///
/// This row is the *temporal record* only - the durable identity every
/// relationship's `origin` / `invalidated_by` points at, plus the GM-curated
/// sequence number used by snapshot/diff queries. Sessions-as-pages (prose,
/// prep, journal), ordinal auto-assignment, and the reorder operation are
/// deferred. See `docs/plans/2026-04-10-entity-relationship-temporal-model.md`.
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
}

/// No relations yet: the `page_id` link (sessions-as-pages) and the inbound
/// relationship edges land in later slices.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
