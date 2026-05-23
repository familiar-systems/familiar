use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Toc {
    Table,
    Id,
    Snapshot,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Toc::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Toc::Id)
                            .integer()
                            .not_null()
                            .primary_key()
                            .check(Expr::col(Toc::Id).eq(1)),
                    )
                    .col(ColumnDef::new(Toc::Snapshot).blob().not_null())
                    .col(
                        ColumnDef::new(Toc::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Toc::Table).to_owned())
            .await
    }
}
