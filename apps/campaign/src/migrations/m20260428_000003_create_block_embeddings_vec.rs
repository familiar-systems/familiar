use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // vec0 is a virtual table — sea-query's schema builder can't model it,
        // so we emit raw SQL. `block_id` is the primary key (vec0 accepts TEXT
        // PKs); the ULID Crockford base32 form is the same encoding the
        // regular `blocks.id` column uses, so the embedding's PK is
        // structurally the same value as the block's PK.
        //
        // `+status` is an auxiliary column with KNN-time filtering: vec0
        // pre-filters on it during the search rather than over-fetching and
        // filtering after, which keeps recall correct when a corner of the
        // graph is GM-only-heavy.
        //
        // Dim 8 is intentional: this is a spike, real embeddings (1536 etc.)
        // would just slow down the test loop without changing the wiring.
        manager
            .get_connection()
            .execute_unprepared(
                "CREATE VIRTUAL TABLE block_embeddings USING vec0(
                    block_id  TEXT PRIMARY KEY,
                    embedding FLOAT[8],
                    +status   TEXT
                );",
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE block_embeddings;")
            .await?;
        Ok(())
    }
}
