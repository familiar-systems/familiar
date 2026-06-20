use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum Relationships {
    Table,
    Id,
    PageA,
    PageB,
    PredicateAToB,
    PredicateBToA,
    Visibility,
    OriginSessionId,
    CreatedAt,
    InvalidationReason,
    InvalidatedBySessionId,
    InvalidatedAt,
}

#[derive(DeriveIden)]
enum Pages {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum Sessions {
    Table,
    Id,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Relationships::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Relationships::Id)
                            .text()
                            .not_null()
                            .primary_key(),
                    )
                    // The two endpoints. Stored canonically (`page_a` < `page_b`
                    // lexicographically) by the owning actor, with the predicate
                    // pair assigned to match, so each fact has exactly one encoding
                    // and a reversed duplicate is structurally impossible.
                    // `ON DELETE CASCADE`: an edge can't outlive an endpoint page.
                    .col(ColumnDef::new(Relationships::PageA).text().not_null())
                    .col(ColumnDef::new(Relationships::PageB).text().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::PageA)
                            .to(Pages::Table, Pages::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::PageB)
                            .to(Pages::Table, Pages::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    // Immutable predicate at each end. A relationship is born with
                    // its predicates and dies with them; evolution creates a new
                    // row (supersede) rather than mutating these.
                    .col(
                        ColumnDef::new(Relationships::PredicateAToB)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(Relationships::PredicateBToA)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(Relationships::Visibility).text().not_null())
                    // Origin = { Prior, Session(FK) } encoded as a nullable session
                    // FK: NULL means `Prior` (true before the campaign began). No
                    // `on_delete` (NO ACTION): `SetNull` would silently rewrite a
                    // Session-origin fact into a Prior one, corrupting provenance.
                    .col(ColumnDef::new(Relationships::OriginSessionId).text().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::OriginSessionId)
                            .to(Sessions::Table, Sessions::Id),
                    )
                    .col(
                        ColumnDef::new(Relationships::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // Invalidation, encoded as a sum type over three columns. The
                    // *presence* of `invalidation_reason` is the live/invalidated
                    // discriminant: NULL = live; set = invalidated. Within an
                    // invalidated row, `invalidated_by_session_id` carries the
                    // prior-vs-session axis (NULL = ended before the campaign began,
                    // Some = ended at that session) - symmetric with `origin`.
                    .col(
                        ColumnDef::new(Relationships::InvalidationReason)
                            .text()
                            .null(),
                    )
                    // No `on_delete` (NO ACTION): NULL here legitimately means
                    // "ended in prior time", so `SetNull` would corrupt
                    // "ended at session N" into that.
                    .col(
                        ColumnDef::new(Relationships::InvalidatedBySessionId)
                            .text()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::InvalidatedBySessionId)
                            .to(Sessions::Table, Sessions::Id),
                    )
                    .col(
                        ColumnDef::new(Relationships::InvalidatedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    // Defense in depth: reason and audit-timestamp co-occur, so the
                    // invalidation state can't go half-set at rest.
                    // `invalidated_by_session_id` stays independently nullable to
                    // carry the prior-vs-session axis. schema.rs ignores CHECKs
                    // (migration-owned); the live/invalidated semantics are covered
                    // behaviorally in `tests/relationships_test.rs`.
                    .check(
                        Expr::col(Relationships::InvalidationReason)
                            .is_null()
                            .eq(Expr::col(Relationships::InvalidatedAt).is_null()),
                    )
                    .to_owned(),
            )
            .await?;

        // One *live* row per canonical fact, while superseded/retconned history
        // coexists. A composite PARTIAL unique index keyed on the live
        // discriminant (`invalidation_reason IS NULL`) - the only correct
        // encoding: End/Create-again must let an invalidated row and a fresh live
        // row share the same predicate pair. sea-orm entities can't express a
        // partial or composite unique index, so this is invisible to schema.rs by
        // design (filtered there alongside CHECKs) and covered behaviorally in
        // `tests/relationships_test.rs`.
        manager
            .create_index(
                Index::create()
                    .name("idx_relationships_live_fact_unique")
                    .table(Relationships::Table)
                    .col(Relationships::PageA)
                    .col(Relationships::PageB)
                    .col(Relationships::PredicateAToB)
                    .col(Relationships::PredicateBToA)
                    .and_where(Expr::col(Relationships::InvalidationReason).is_null())
                    .unique()
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Dropping the table drops its indexes (SQLite).
        manager
            .drop_table(Table::drop().table(Relationships::Table).to_owned())
            .await
    }
}
