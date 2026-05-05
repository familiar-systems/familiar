use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Blocks {
    Table,
    Id,
    ThingId,
    Status,
    Ordering,
    Body,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Things {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Blocks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Blocks::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Blocks::ThingId).text().not_null())
                    .col(ColumnDef::new(Blocks::Status).text().not_null())
                    .col(ColumnDef::new(Blocks::Ordering).big_integer().not_null())
                    .col(ColumnDef::new(Blocks::Body).text().not_null())
                    .col(
                        ColumnDef::new(Blocks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Blocks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Blocks::Table, Blocks::ThingId)
                            .to(Things::Table, Things::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Blocks::Table).to_owned())
            .await
    }
}
