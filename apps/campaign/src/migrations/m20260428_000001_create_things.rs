use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Things {
    Table,
    Id,
    Name,
    Status,
    PrototypeId,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Things::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Things::Id).text().not_null().primary_key())
                    .col(ColumnDef::new(Things::Name).text().not_null())
                    .col(ColumnDef::new(Things::Status).text().not_null())
                    .col(ColumnDef::new(Things::PrototypeId).text().null())
                    .col(
                        ColumnDef::new(Things::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Things::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Things::Table, Things::PrototypeId)
                            .to(Things::Table, Things::Id),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Things::Table).to_owned())
            .await
    }
}
