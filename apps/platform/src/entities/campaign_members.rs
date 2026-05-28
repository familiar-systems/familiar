use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use uuid::Uuid;

/// Membership link between a user and a campaign.
///
/// Composite primary key `(campaign_id, user_id)`. The `role` column
/// stores `"gm"` or `"player"`, mapped to [`CampaignRole`] at the
/// handler boundary.
///
/// [`CampaignRole`]: familiar_systems_app_shared::campaigns::internal::CampaignRole
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "campaign_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub campaign_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::campaigns::Entity",
        from = "Column::CampaignId",
        to = "super::campaigns::Column::Id"
    )]
    Campaign,
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::UserId",
        to = "super::users::Column::Id"
    )]
    User,
}

impl Related<super::campaigns::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Campaign.def()
    }
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
