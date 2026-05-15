use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::Serialize;

/// Idempotency record for `POST /api/campaigns`.
///
/// PK on `idempotency_token` (SPA-minted nanoid) makes the upsert race-safe:
/// the loser of a concurrent insert sees a unique-violation, re-reads the
/// row, and uses the winner's `campaign_id`.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "create_attempts")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub idempotency_token: String,
    pub campaign_id: String,
    pub created_at: DateTime<Utc>,
}

// No DB-level relation: see migration's commentary. The (token -> campaign_id)
// link is logical only.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
