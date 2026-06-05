use sea_orm_migration::prelude::*;

mod m20260417_000001_create_users;
mod m20260515_000001_create_campaigns;
mod m20260515_000002_create_create_attempts;
pub mod m20260528_000001_create_campaign_members;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260417_000001_create_users::Migration),
            Box::new(m20260515_000001_create_campaigns::Migration),
            Box::new(m20260515_000002_create_create_attempts::Migration),
            Box::new(m20260528_000001_create_campaign_members::Migration),
        ]
    }
}
