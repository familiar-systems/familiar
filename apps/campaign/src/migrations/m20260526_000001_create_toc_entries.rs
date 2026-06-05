use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum TocEntries {
    Table,
    Id,
    PageId,
    FolderTitle,
    Visibility,
    ParentId,
    Position,
}

#[derive(DeriveIden)]
enum Pages {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(TocEntries::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TocEntries::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TocEntries::PageId).text().null())
                    .col(ColumnDef::new(TocEntries::FolderTitle).text().null())
                    .col(ColumnDef::new(TocEntries::Visibility).text().not_null())
                    .col(ColumnDef::new(TocEntries::ParentId).text().null())
                    .col(ColumnDef::new(TocEntries::Position).integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(TocEntries::Table, TocEntries::PageId)
                            .to(Pages::Table, Pages::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(TocEntries::Table).to_owned())
            .await
    }
}
