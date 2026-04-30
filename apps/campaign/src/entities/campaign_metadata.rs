use chrono::{DateTime, Utc};
use familiar_systems_app_shared::id::CampaignId;
use sea_orm::entity::prelude::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash, DeriveValueType)]
pub struct CampaignIdCol(pub String);

impl From<CampaignId> for CampaignIdCol {
    fn from(id: CampaignId) -> Self {
        Self(id.0)
    }
}

impl From<CampaignIdCol> for CampaignId {
    fn from(col: CampaignIdCol) -> Self {
        Self(col.0)
    }
}

#[derive(Debug, Clone, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "campaign_metadata")]
pub struct Model {
    // The ID is always 1 since we always have a single campaign metadata record
    #[sea_orm(primary_key, auto_increment = false, extra = "CHECK (id = 1)")]
    pub id: i32,
    pub campaign_id: CampaignIdCol,
    pub name: String,
    pub description: Option<String>,
    pub start_date: DateTime<Utc>,
    pub end_date: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
