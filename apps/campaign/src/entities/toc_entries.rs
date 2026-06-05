//! Relational persistence for the Table of Contents tree.
//!
//! The ToC is a nested tree of Folders and Pages backed by a LoroTree CRDT
//! during an active session. This table is the at-rest source of truth:
//! on checkout, `restore_toc` rebuilds the CRDT from these rows; on
//! writeback, `snapshot_toc` replaces all rows from the current CRDT state.
//!
//! ## Writeback strategy
//!
//! Upsert: `DELETE WHERE id NOT IN (...) + INSERT OR REPLACE`. Row IDs
//! are stable across writeback cycles; the TocActor maintains a
//! `HashMap<TreeID, String>` mapping Loro node IDs to row ULIDs. This
//! avoids rewriting the entire table on every debounce tick when only a
//! few entries changed, reducing aggregate WAL churn at scale. No
//! timestamps because the table is a positional index, not an entity
//! with lifecycle.
//!
//! ## Invariant: exactly one of `page_id` / `folder_title` is non-null
//!
//! A row is either a Page pointer (page_id set, folder_title null) or a
//! Folder (folder_title set, page_id null). The application enforces this
//! in `snapshot_toc`; sea-orm entities can't express CHECK constraints, so
//! it is not enforced at the DB level.
//!
//! ## Page titles are not stored here
//!
//! Page entries derive their display title from `pages.name` via a join
//! during restore. This avoids drift between the ToC and the Page's own
//! name. Only Folder titles live in this table (folders have no other home
//! for their name).

use sea_orm::entity::prelude::*;

use crate::entities::columns::{PageIdCol, StatusCol};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "toc_entries")]
pub struct Model {
    /// ULID surrogate. Stable across writeback cycles for upsert matching.
    /// Not referenced by anything outside this table.
    #[sea_orm(primary_key, auto_increment = false, column_type = "Text")]
    pub id: String,
    /// FK to `pages.id`. Non-null for Page entries; null for Folders.
    /// CASCADE delete: if a Page is removed from the DB between sessions,
    /// its ToC row disappears automatically.
    pub page_id: Option<PageIdCol>,
    /// Display title for Folder entries only. Null when `page_id` is set
    /// (Page titles come from `pages.name` on restore).
    #[sea_orm(column_type = "Text")]
    pub folder_title: Option<String>,
    pub visibility: StatusCol,
    /// References `id` of the parent row (null = root level). No FK
    /// constraint: full-replace writeback makes self-referential cascades
    /// painful for no benefit; integrity is guaranteed by construction.
    #[sea_orm(column_type = "Text")]
    pub parent_id: Option<String>,
    /// Sibling order within the parent group (0-indexed).
    pub position: i32,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::pages::Entity",
        from = "Column::PageId",
        to = "super::pages::Column::Id",
        on_delete = "Cascade"
    )]
    Page,
}

impl Related<super::pages::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Page.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
