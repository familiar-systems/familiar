use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use uuid::Uuid;

/// Membership link between a user and a campaign.
///
/// Composite primary key `(campaign_id, user_id)`. The `role` column is a
/// [`CampaignRole`], persisted as `"gm"` / `"player"` text and guarded at the
/// DB level by `CHECK (role IN ('gm','player'))` (see the create migration).
/// It converts into the wire-facing [app `CampaignRole`] at the handler
/// boundary.
///
/// [app `CampaignRole`]: familiar_systems_app_shared::campaigns::internal::CampaignRole
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "campaign_members")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub campaign_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: Uuid,
    pub role: CampaignRole,
    pub created_at: DateTime<Utc>,
}

/// A member's functional role but for ORM internal usage.
///
/// Must have a 1:1 mapping in a few places:
///
/// 1. Here (obviously)
/// 2. [`CampaignRole`](familiar_systems_app_shared::campaigns::internal::CampaignRole)
/// 3. Migrations overall, specifically:
///    a. [`m20260528_000001_create_campaign_members`](crate::migrations::m20260528_000001_create_campaign_members)
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "String(StringLen::None)")]
pub enum CampaignRole {
    #[sea_orm(string_value = "gm")]
    Gm,
    #[sea_orm(string_value = "player")]
    Player,
}

impl From<CampaignRole> for familiar_systems_app_shared::campaigns::internal::CampaignRole {
    fn from(role: CampaignRole) -> Self {
        match role {
            CampaignRole::Gm => Self::Gm,
            CampaignRole::Player => Self::Player,
        }
    }
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
