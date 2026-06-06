use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Pages {
    Table,
    Id,
    Name,
    Status,
    Kind,
    TemplateId,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Pages::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Pages::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Pages::Name).text().not_null())
                    .col(ColumnDef::new(Pages::Status).text().not_null())
                    .col(ColumnDef::new(Pages::Kind).text().not_null())
                    .col(ColumnDef::new(Pages::TemplateId).text().null())
                    .col(
                        ColumnDef::new(Pages::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Pages::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Pages::Table, Pages::TemplateId)
                            .to(Pages::Table, Pages::Id),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Pages::Table).to_owned())
            .await
    }
}
