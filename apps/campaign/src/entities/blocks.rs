use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::{BlockIdCol, StatusCol, ThingIdCol};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "blocks")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: BlockIdCol,
    pub thing_id: ThingIdCol,
    pub status: StatusCol,
    pub ordering: i64,
    #[sea_orm(column_type = "Text")]
    pub body: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::things::Entity",
        from = "Column::ThingId",
        to = "super::things::Column::Id",
        on_delete = "Cascade"
    )]
    Thing,
}

impl Related<super::things::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Thing.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
