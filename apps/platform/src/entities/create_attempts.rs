use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// Idempotency record for `POST /api/campaigns`.
///
/// PK on `idempotency_token` (SPA-minted nanoid) makes the upsert race-safe:
/// the loser of a concurrent insert sees a unique-violation, re-reads the
/// row, and uses the winner's `campaign_id`.
///
/// **No FK to `campaigns.id`.** The route writes the `create_attempts` row
/// *before* it calls the shard and *before* it inserts the `campaigns` row,
/// because that ordering is what makes retries safe (see the numbered steps
/// in `routes/campaigns.rs::create_campaign`). An FK here would block the
/// insert with a constraint failure since the `campaigns` row doesn't exist
/// yet. The relationship is logical only.
///
/// TODO: Orphaned rows (token written campaign never created) are harmless;
/// a future vacuum job could prune rows older than XX days.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "create_attempts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub idempotency_token: String,
    pub campaign_id: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
