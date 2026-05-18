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
