use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[derive(DeriveIden)]
enum CampaignMetadata {
    Table,
    Id,
    CampaignId,
    Name,
    Description,
    CreatedAt,
    UpdatedAt,
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(CampaignMetadata::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(CampaignMetadata::Id)
                            .integer()
                            .not_null()
                            .primary_key()
                            .check(Expr::col(CampaignMetadata::Id).eq(1)),
                    )
                    .col(
                        ColumnDef::new(CampaignMetadata::CampaignId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(CampaignMetadata::Name).text().not_null())
                    .col(ColumnDef::new(CampaignMetadata::Description).text())
                    .col(
                        ColumnDef::new(CampaignMetadata::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(CampaignMetadata::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(CampaignMetadata::Table).to_owned())
            .await
    }
}

#[cfg(test)]
mod tests {
    //! Migration-specific invariants for `campaign_metadata`. Generic
    //! "entity matches migrated schema" coverage lives in
    //! `tests/schema_drift.rs`.

    use chrono::Utc;
    use familiar_systems_app_shared::id::CampaignId;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, Set};
    use sea_orm_migration::MigratorTrait;

    use crate::db;
    use crate::entities::campaign_metadata;
    use crate::migrations::Migrator;

    async fn setup() -> DatabaseConnection {
        db::register_sqlite_vec();
        let db = db::connect("sqlite::memory:").await.expect("connect");
        Migrator::up(&db, None).await.expect("migrate");
        db
    }

    fn row(id: i32) -> campaign_metadata::ActiveModel {
        let now = Utc::now();
        campaign_metadata::ActiveModel {
            id: Set(id),
            campaign_id: Set(CampaignId::new().into()),
            name: Set("Curse of Strahd".into()),
            description: Set(Some("Gothic horror in Barovia".into())),
            created_at: Set(now),
            updated_at: Set(now),
        }
    }

    #[tokio::test]
    async fn id_one_round_trips_every_column() {
        let db = setup().await;
        let written = row(1);
        let written_campaign_id = written.campaign_id.clone().unwrap();
        let written_name = written.name.clone().unwrap();
        let written_description = written.description.clone().unwrap();
        let written_created = written.created_at.clone().unwrap();
        let written_updated = written.updated_at.clone().unwrap();

        written.insert(&db).await.expect("id=1 should insert");

        let read = campaign_metadata::Entity::find_by_id(1)
            .one(&db)
            .await
            .expect("find_by_id")
            .expect("row exists");

        assert_eq!(read.id, 1);
        assert_eq!(read.campaign_id, written_campaign_id);
        assert_eq!(read.name, written_name);
        assert_eq!(read.description, written_description);
        assert_eq!(read.created_at, written_created);
        assert_eq!(read.updated_at, written_updated);
    }

    #[tokio::test]
    async fn id_other_than_one_violates_check_constraint() {
        let db = setup().await;

        let err = row(2)
            .insert(&db)
            .await
            .expect_err("id=2 must be rejected by CHECK (id = 1)");

        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("check"),
            "expected CHECK constraint violation, got: {err}"
        );
    }

    #[tokio::test]
    async fn second_insert_with_id_one_violates_primary_key() {
        let db = setup().await;

        row(1).insert(&db).await.expect("first id=1 should insert");
        let err = row(1)
            .insert(&db)
            .await
            .expect_err("duplicate id=1 must be rejected by PRIMARY KEY");

        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("unique") || msg.contains("primary"),
            "expected primary-key violation, got: {err}"
        );
    }
}
