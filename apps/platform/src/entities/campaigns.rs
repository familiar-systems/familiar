use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Platform's view of a campaign.
///
/// `id` is a Nanoid (TEXT) minted by the platform. `shard_url` is the
/// campaign-tier URL that hosts this campaign's per-campaign DB; future
/// requests for this campaign route to that shard.
///
/// The four nullable mirror columns plus `wizard_completed_at` are populated
/// by the campaign tier on successful initialize via a metadata mirror call.
/// Until that lands, the SPA renders an "Untitled campaign" placeholder.
///
/// `last_init_error` is set by the campaign tier when an initialize attempt
/// fails (`POST /internal/platform/campaigns/<id>/init-failed`). Distinct
/// from "no init attempt yet" (`wizard_completed_at IS NULL` AND
/// `last_init_error IS NULL`).
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "campaigns")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub owner_user_id: Uuid,
    pub shard_url: String,
    pub name: Option<String>,
    pub tagline: Option<String>,
    pub game_system: Option<String>,
    pub content_locale: Option<String>,
    pub last_init_error: Option<String>,
    pub wizard_completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::users::Entity",
        from = "Column::OwnerUserId",
        to = "super::users::Column::Id"
    )]
    Owner,
}

impl Related<super::users::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Owner.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveValue::Unchanged;

    #[test]
    fn model_into_active_model_roundtrip() {
        let now = Utc::now();
        let id = "test-nanoid".to_string();
        let owner = Uuid::now_v7();
        let m = Model {
            id: id.clone(),
            owner_user_id: owner,
            shard_url: "http://localhost:3001".into(),
            name: None,
            tagline: None,
            game_system: None,
            content_locale: None,
            last_init_error: None,
            wizard_completed_at: None,
            created_at: now,
            updated_at: now,
        };
        let am: ActiveModel = m.clone().into();
        assert_eq!(am.id, Unchanged(id));
        assert_eq!(am.owner_user_id, Unchanged(owner));
        assert_eq!(am.created_at, Unchanged(now));
        assert_eq!(am.last_init_error, Unchanged(None));
    }
}
