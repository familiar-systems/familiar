use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Id,
    Ordinal,
    CreatedAt,
    UpdatedAt,
    PageId,
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
                    .table(Sessions::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Sessions::Id).text().not_null().primary_key())
                    // GM-curated "Session N". Unique within the campaign; the
                    // unique constraint is an inline column key (a `'u'` SQLite
                    // auto-index), declared identically by the entity's
                    // `#[sea_orm(unique)]`. `schema_drift` compares only explicit
                    // (`'c'`) indexes, so the two stay in lockstep through the
                    // column definition, not an index comparison.
                    //
                    // CHECK (ordinal >= 0): forbids negative session numbers but
                    // allows Session 0 (a real "Session Zero": setup / character
                    // creation). "Before any session" is Prior (a NULL origin on
                    // the relationship), not a negative ordinal. `schema_drift`
                    // can't see CHECKs; this one is non-load-bearing (assignment
                    // is `MAX+1`, never < 0), so it ships without a guard test.
                    .col(
                        ColumnDef::new(Sessions::Ordinal)
                            .big_integer()
                            .not_null()
                            .unique_key()
                            .check(Expr::col(Sessions::Ordinal).gte(0)),
                    )
                    .col(
                        ColumnDef::new(Sessions::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // Last mutation (e.g. an ordinal reorder); equals `created_at`
                    // at genesis. Distinct from the immutable `created_at`
                    // recording-order axis, mirroring `pages`/`blocks`.
                    .col(
                        ColumnDef::new(Sessions::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // The Session page this documents (sessions-as-pages). Nullable:
                    // the temporal record is the durable half and may exist (or
                    // outlive its page) without one. `unique_key` enforces one
                    // session per page - an inline `'u'` auto-index like `ordinal`,
                    // which `schema_drift` ignores while the entity mirrors it via
                    // `#[sea_orm(unique)]` (SQLite permits many NULLs under a unique
                    // index, so page-less sessions are unconstrained).
                    .col(ColumnDef::new(Sessions::PageId).text().null().unique_key())
                    // `ON DELETE SET NULL`: the temporal row outlives its page so
                    // relationship provenance survives a page deletion.
                    .foreign_key(
                        ForeignKey::create()
                            .from(Sessions::Table, Sessions::PageId)
                            .to(Pages::Table, Pages::Id)
                            .on_delete(ForeignKeyAction::SetNull),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Sessions::Table).to_owned())
            .await
    }
}
