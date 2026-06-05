use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum CampaignMembers {
    Table,
    CampaignId,
    UserId,
    Role,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Campaigns {
    Table,
    Id,
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
                    .table(CampaignMembers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CampaignMembers::CampaignId)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CampaignMembers::UserId).uuid().not_null())
                    .col(
                        ColumnDef::new(CampaignMembers::Role)
                            .string()
                            .not_null()
                            .check(Expr::col(CampaignMembers::Role).is_in(["gm", "player"])),
                    )
                    .col(
                        ColumnDef::new(CampaignMembers::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .primary_key(
                        Index::create()
                            .col(CampaignMembers::CampaignId)
                            .col(CampaignMembers::UserId),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_campaign_members_campaign_id")
                            .from(CampaignMembers::Table, CampaignMembers::CampaignId)
                            .to(Campaigns::Table, Campaigns::Id),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_campaign_members_user_id")
                            .from(CampaignMembers::Table, CampaignMembers::UserId)
                            .to(Users::Table, Users::Id),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_campaign_members_user_id")
                    .table(CampaignMembers::Table)
                    .col(CampaignMembers::UserId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CampaignMembers::Table).to_owned())
            .await
    }
}
