use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::{PageIdCol, PageKindCol, StatusCol};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "pages")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: PageIdCol,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    pub status: StatusCol,
    /// Which `PageKind` this page is (entity, template, session). Default `entity`.
    pub kind: PageKindCol,
    /// Which template this was cloned from, if any.
    pub template_id: Option<PageIdCol>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::blocks::Entity")]
    Blocks,
    #[sea_orm(belongs_to = "Entity", from = "Column::TemplateId", to = "Column::Id")]
    Template,
}

impl Related<super::blocks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Blocks.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
