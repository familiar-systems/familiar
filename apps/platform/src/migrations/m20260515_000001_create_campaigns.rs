use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Campaigns {
    Table,
    Id,
    OwnerUserId,
    ShardUrl,
    Name,
    Tagline,
    GameSystem,
    ContentLocale,
    LastInitError,
    WizardCompletedAt,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Campaigns::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Campaigns::Id)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Campaigns::OwnerUserId).uuid().not_null())
                    .col(ColumnDef::new(Campaigns::ShardUrl).string().not_null())
                    .col(ColumnDef::new(Campaigns::Name).string().null())
                    .col(ColumnDef::new(Campaigns::Tagline).string().null())
                    .col(ColumnDef::new(Campaigns::GameSystem).string().null())
                    .col(ColumnDef::new(Campaigns::ContentLocale).string().null())
                    .col(ColumnDef::new(Campaigns::LastInitError).string().null())
                    .col(
                        ColumnDef::new(Campaigns::WizardCompletedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(Campaigns::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Campaigns::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_campaigns_owner_user_id")
                            .from(Campaigns::Table, Campaigns::OwnerUserId)
                            .to(Users::Table, Users::Id),
                    )
                    .to_owned(),
            )
            .await?;

        // Owner index supports the `GET /api/campaigns` query
        // (`WHERE owner_user_id = ? ORDER BY created_at DESC`).
        manager
            .create_index(
                Index::create()
                    .name("idx_campaigns_owner_user_id")
                    .table(Campaigns::Table)
                    .col(Campaigns::OwnerUserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Campaigns::Table).to_owned())
            .await
    }
}
