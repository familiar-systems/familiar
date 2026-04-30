use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum CampaignMetadata {
    Table,
    Id,
    CampaignId,
    Name,
    Description,
    StartData,
    EndDate,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CampaignMetadata::Table)
                    .col(
                        ColumnDef::new(CampaignMetadata::Id)
                            .integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(CampaignMetadata::CampaignId)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CampaignMetadata::Name).string().not_null())
                    .col(ColumnDef::new(CampaignMetadata::Description).string())
                    .col(ColumnDef::new(CampaignMetadata::StartData).date())
                    .col(ColumnDef::new(CampaignMetadata::EndDate).date())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CampaignMetadata::Table).to_owned())
            .await
    }
}
