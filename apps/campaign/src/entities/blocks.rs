use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::{BlockIdCol, PageIdCol, SectionCol, StatusCol};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "blocks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: BlockIdCol,
    pub page_id: PageIdCol,
    pub status: StatusCol,
    pub ordering: i64,
    #[sea_orm(column_type = "Blob")]
    pub content: Vec<u8>,
    pub section: SectionCol,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
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
