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
    // Factuality axis: when the fact was true in the fiction, [origin, superseded).
    OriginSessionId,
    SupersededSessionId,
    RetconSessionId,
    // Knowledge axis: whether/when the players learned it.
    IsSecret,
    RevealSessionId,
    CreatedAt,
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
                    // Factuality. Three nullable session FKs; their NULLs are
                    // load-bearing, so all three are NO ACTION (a `SetNull` cascade
                    // would silently rewrite provenance):
                    //   origin     NULL = Prior (true before the campaign began).
                    //   superseded NULL = still true. A non-NULL value is also the
                    //              live/ended discriminant (see the partial index):
                    //              ending is always *at a session*, never in prior.
                    //   retcon     NULL = not retconned.
                    .col(ColumnDef::new(Relationships::OriginSessionId).text().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::OriginSessionId)
                            .to(Sessions::Table, Sessions::Id),
                    )
                    .col(
                        ColumnDef::new(Relationships::SupersededSessionId)
                            .text()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::SupersededSessionId)
                            .to(Sessions::Table, Sessions::Id),
                    )
                    .col(ColumnDef::new(Relationships::RetconSessionId).text().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::RetconSessionId)
                            .to(Sessions::Table, Sessions::Id),
                    )
                    // Knowledge. `is_secret = false` is public (always known);
                    // `reveal_session_id` is the session the players learned a secret
                    // fact (NULL = not yet revealed). Both are freely mutable (the GM
                    // can reveal, conceal, or re-hide). NO ACTION for the same reason as
                    // the factuality FKs.
                    .col(ColumnDef::new(Relationships::IsSecret).boolean().not_null())
                    .col(ColumnDef::new(Relationships::RevealSessionId).text().null())
                    .foreign_key(
                        ForeignKey::create()
                            .from(Relationships::Table, Relationships::RevealSessionId)
                            .to(Sessions::Table, Sessions::Id),
                    )
                    .col(
                        ColumnDef::new(Relationships::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    // A public fact has no reveal event: only a secret fact can carry a
                    // reveal. (is_secret) OR (reveal IS NULL). The `SetKnowledge` write
                    // always sets the pair from a `Knowledge` value, so the illegal
                    // combo is unreachable; the CHECK is defense in depth. schema.rs
                    // ignores CHECKs (migration-owned); covered behaviorally in
                    // `tests/relationships_test.rs`.
                    .check(
                        Expr::col(Relationships::IsSecret)
                            .eq(true)
                            .or(Expr::col(Relationships::RevealSessionId).is_null()),
                    )
                    .to_owned(),
            )
            .await?;

        // One *live* row per canonical fact, while superseded/retconned history
        // coexists. Liveness is factuality only: a row is live iff it is neither
        // superseded nor retconned, so the partial index keys on both. (Knowledge
        // is excluded: two currently-true rows of the same fact are a duplicate
        // regardless of who knows them.) sea-orm entities can't express a partial
        // or composite unique index, so this is invisible to schema.rs by design
        // (filtered there alongside CHECKs) and covered behaviorally in
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
                    .and_where(Expr::col(Relationships::SupersededSessionId).is_null())
                    .and_where(Expr::col(Relationships::RetconSessionId).is_null())
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
