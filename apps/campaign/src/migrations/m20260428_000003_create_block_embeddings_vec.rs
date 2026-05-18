use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // vec0 is a virtual table that sea-query's schema builder can't model.
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
