use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{ActorRef, Spawn};
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait};
use sea_orm_migration::MigratorTrait;

use crate::actors::database_writer::{DatabaseActor, DatabaseActorArgs};
use crate::config::{Config, StorageBackend};
use crate::db;
use crate::entities::campaign_metadata;
use crate::error::InitError;
use crate::migrations::Migrator;

use super::store::CampaignStore;
use super::store_local::LocalCampaignStore;

pub struct CampaignDatabase {
    reader: DatabaseConnection,
    writer: ActorRef<DatabaseActor>,
    path: PathBuf,
}

impl CampaignDatabase {
    pub async fn checkout(
        store: &dyn CampaignStore,
        campaign_id: &CampaignId,
        owner_user_id: Option<&UserId>,
    ) -> Result<Self, InitError> {
        let started = Instant::now();
        tracing::info!("checking out campaign database");

        let path = store
            .checkout(campaign_id)
            .await
            .map_err(|source| InitError::Checkout {
                campaign_id: campaign_id.clone(),
                source,
            })?;

        let database_url = format!("sqlite://{}?mode=rwc", path.display());
        let conn = db::connect(&database_url)
            .await
            .map_err(|source| InitError::OpenDatabase {
                path: path.clone(),
                source,
            })?;

        Migrator::up(&conn, None)
            .await
            .map_err(InitError::Migration)?;

        let existing = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&conn)
            .await
            .map_err(InitError::Migration)?;

        if existing.is_none() {
            let now = Utc::now();
            campaign_metadata::ActiveModel {
                id: Set(campaign_metadata::METADATA_ROW_ID),
                campaign_id: Set(campaign_id.clone().into()),
                owner_user_id: Set(owner_user_id.map(|u| u.0.to_string()).unwrap_or_default()),
                name: Set("Untitled".into()),
                tagline: Set(None),
                game_system: Set(None),
                content_locale: Set(None),
                wizard_completed_at: Set(None),
                created_at: Set(now),
                updated_at: Set(now),
            }
            .insert(&conn)
            .await
            .map_err(InitError::Migration)?;
        }

        let reader =
            db::connect_readonly(&path)
                .await
                .map_err(|source| InitError::OpenDatabase {
                    path: path.clone(),
                    source,
                })?;

        let writer = DatabaseActor::spawn(DatabaseActorArgs {
            campaign_id: campaign_id.clone(),
            conn,
        });

        tracing::info!(
            checkout_elapsed_ms = started.elapsed().as_millis() as u64,
            "campaign database ready"
        );

        Ok(Self {
            reader,
            writer,
            path,
        })
    }

    pub fn reader(&self) -> &DatabaseConnection {
        &self.reader
    }

    pub fn writer(&self) -> &ActorRef<DatabaseActor> {
        &self.writer
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub async fn release(
        self,
        store: &dyn CampaignStore,
        campaign_id: &CampaignId,
    ) -> Result<(), InitError> {
        if let Err(e) = self.writer.stop_gracefully().await {
            tracing::warn!(
                ?e,
                actor_kind = "database_actor",
                "actor already stopped during release"
            );
        }
        self.writer.wait_for_shutdown_with_result(|_| ()).await;

        store
            .release(campaign_id, &self.path)
            .await
            .map_err(|source| InitError::Release {
                campaign_id: campaign_id.clone(),
                source,
            })?;

        Ok(())
    }
}

pub fn store_from_config(config: &Config) -> Arc<dyn CampaignStore> {
    match config.storage_backend {
        StorageBackend::Local => {
            Arc::new(LocalCampaignStore::new(config.campaign_data_dir.clone()))
        }
        StorageBackend::S3 => {
            todo!("S3 store implementation lands in a follow-up")
        }
    }
}
