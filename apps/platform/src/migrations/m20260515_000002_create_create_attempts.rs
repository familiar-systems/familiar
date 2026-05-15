use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum CreateAttempts {
    Table,
    IdempotencyToken,
    CampaignId,
    CreatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    // Idempotency table for `POST /api/campaigns`. The SPA mints a token per
    // create-click; the platform's first response writes
    // `(token -> campaign_id)` and any retry of the same logical call returns
    // the same `campaign_id` rather than allocating a new one.
    //
    // PK on `idempotency_token` makes the upsert race-safe: the loser of a
    // concurrent insert sees a unique-violation, re-reads the row, and uses
    // the winner's `campaign_id`. No status column needed.
    //
    // **No FK to `campaigns.id`.** The route writes the `create_attempts` row
    // *before* it calls the shard and *before* it inserts the `campaigns`
    // row, because that ordering is what makes retries safe — see the
    // numbered steps in `apps/platform/src/routes/campaigns.rs::create_campaign`.
    // An FK here would block step 3 with a constraint failure since the
    // `campaigns` row doesn't exist yet. The relationship is logical only;
    // a vacuum job can prune orphans (rows older than ~30 days; not
    // load-bearing for correctness).
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CreateAttempts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CreateAttempts::IdempotencyToken)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CreateAttempts::CampaignId)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CreateAttempts::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CreateAttempts::Table).to_owned())
            .await
    }
}
