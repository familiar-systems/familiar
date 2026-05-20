use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum CampaignMetadata {
    Table,
    GameSystem,
    ContentLocale,
    WizardCompletedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("ALTER TABLE campaign_metadata RENAME COLUMN description TO tagline")
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(CampaignMetadata::Table)
                    .add_column(ColumnDef::new(CampaignMetadata::GameSystem).text())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(CampaignMetadata::Table)
                    .add_column(ColumnDef::new(CampaignMetadata::ContentLocale).text())
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(CampaignMetadata::Table)
                    .add_column(
                        ColumnDef::new(CampaignMetadata::WizardCompletedAt)
                            .timestamp_with_time_zone(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("ALTER TABLE campaign_metadata RENAME COLUMN tagline TO description")
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(CampaignMetadata::Table)
                    .drop_column(CampaignMetadata::GameSystem)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(CampaignMetadata::Table)
                    .drop_column(CampaignMetadata::ContentLocale)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(CampaignMetadata::Table)
                    .drop_column(CampaignMetadata::WizardCompletedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}
