use sea_orm_migration::prelude::*;

mod m20260428_000001_create_things;
mod m20260428_000002_create_blocks;
mod m20260428_000003_create_block_embeddings_vec;
mod m20260430_000001_create_campaign_metadata;
mod m20260519_000001_add_wizard_fields;
mod m20260520_000001_add_owner_user_id;
mod m20260526_000001_create_toc_entries;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260428_000001_create_things::Migration),
            Box::new(m20260428_000002_create_blocks::Migration),
            Box::new(m20260428_000003_create_block_embeddings_vec::Migration),
            Box::new(m20260430_000001_create_campaign_metadata::Migration),
            Box::new(m20260519_000001_add_wizard_fields::Migration),
            Box::new(m20260520_000001_add_owner_user_id::Migration),
            Box::new(m20260526_000001_create_toc_entries::Migration),
        ]
    }
}
