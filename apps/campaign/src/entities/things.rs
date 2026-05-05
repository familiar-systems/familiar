use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::entities::columns::{StatusCol, ThingIdCol};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "things")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: ThingIdCol,
    #[sea_orm(column_type = "Text")]
    pub name: String,
    pub status: StatusCol,
    pub prototype_id: Option<ThingIdCol>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::blocks::Entity")]
    Blocks,
    #[sea_orm(belongs_to = "Entity", from = "Column::PrototypeId", to = "Column::Id")]
    Prototype,
}

impl Related<super::blocks::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Blocks.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
