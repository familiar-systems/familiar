//! `DatabaseActor`: single owner of the per-campaign sea-orm write
//! connection.
//!
//! Every write to the campaign DB flows through this actor's mailbox.

use chrono::Utc;
use familiar_systems_app_shared::id::CampaignId;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, DatabaseConnection, EntityTrait};

use crate::entities::campaign_metadata;

pub struct DatabaseActor {
    campaign_id: CampaignId,
    conn: DatabaseConnection,
}

pub struct DatabaseActorArgs {
    pub campaign_id: CampaignId,
    pub conn: DatabaseConnection,
}

impl Actor for DatabaseActor {
    type Args = DatabaseActorArgs;
    type Error = std::convert::Infallible;

    async fn on_start(
        args: Self::Args,
        _actor_ref: kameo::actor::ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        let _span =
            tracing::info_span!("database_actor", campaign_id = %args.campaign_id.0).entered();
        tracing::debug!("database actor started");
        Ok(Self {
            campaign_id: args.campaign_id,
            conn: args.conn,
        })
    }

    async fn on_stop(
        &mut self,
        _actor_ref: kameo::actor::WeakActorRef<Self>,
        _reason: kameo::error::ActorStopReason,
    ) -> Result<(), Self::Error> {
        tracing::debug!(campaign_id = %self.campaign_id.0, "database actor stopped");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// PatchCampaignMetadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct PatchCampaignMetadata {
    pub name: Option<String>,
    pub tagline: Option<String>,
    pub game_system: Option<String>,
    pub content_locale: Option<String>,
    pub complete_wizard: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PatchCampaignError {
    #[error("wizard already completed")]
    AlreadyInitialized,
    #[error("campaign metadata row missing")]
    NoMetadataRow,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("database actor unavailable")]
    ActorUnavailable,
}

#[derive(Debug, Clone, kameo::Reply)]
pub struct PatchCampaignResult {
    pub model: campaign_metadata::Model,
    pub wizard_just_completed: bool,
}

impl Message<PatchCampaignMetadata> for DatabaseActor {
    type Reply = Result<PatchCampaignResult, PatchCampaignError>;

    async fn handle(
        &mut self,
        msg: PatchCampaignMetadata,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let existing = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&self.conn)
            .await?
            .ok_or(PatchCampaignError::NoMetadataRow)?;

        let already_completed = existing.wizard_completed_at.is_some();
        if msg.complete_wizard && already_completed {
            return Err(PatchCampaignError::AlreadyInitialized);
        }

        let now = Utc::now();
        let mut am: campaign_metadata::ActiveModel = existing.into();

        if let Some(name) = msg.name {
            am.name = Set(name);
        }
        if let Some(tagline) = msg.tagline {
            am.tagline = Set(Some(tagline));
        }
        if let Some(game_system) = msg.game_system {
            am.game_system = Set(Some(game_system));
        }
        if let Some(content_locale) = msg.content_locale {
            am.content_locale = Set(Some(content_locale));
        }

        let wizard_just_completed = msg.complete_wizard && !already_completed;
        if wizard_just_completed {
            am.wizard_completed_at = Set(Some(now));
        }

        am.updated_at = Set(now);
        let model = am.update(&self.conn).await?;

        Ok(PatchCampaignResult {
            model,
            wizard_just_completed,
        })
    }
}

// ---------------------------------------------------------------------------
// GetMetadata
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct GetMetadata;

#[derive(Debug, thiserror::Error)]
pub enum MetadataError {
    #[error("campaign metadata row missing")]
    NoMetadataRow,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
    #[error("database actor unavailable")]
    ActorUnavailable,
}

impl Message<GetMetadata> for DatabaseActor {
    type Reply = Result<campaign_metadata::Model, MetadataError>;

    async fn handle(
        &mut self,
        _: GetMetadata,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&self.conn)
            .await?
            .ok_or(MetadataError::NoMetadataRow)
    }
}

// ---------------------------------------------------------------------------
// Ping (health check / test)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ping;

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub struct Pong;

impl Message<Ping> for DatabaseActor {
    type Reply = Pong;

    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        Pong
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::migrations::Migrator;
    use kameo::actor::Spawn;
    use sea_orm_migration::MigratorTrait;

    async fn spawn_with_migrations() -> (kameo::actor::ActorRef<DatabaseActor>, CampaignId) {
        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite open");
        Migrator::up(&conn, None).await.expect("migrate");

        let campaign_id = CampaignId::generate();

        let now = Utc::now();
        campaign_metadata::ActiveModel {
            id: Set(campaign_metadata::METADATA_ROW_ID),
            campaign_id: Set(campaign_id.clone().into()),
            owner_user_id: Set(String::new()),
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
        .expect("seed metadata");

        let actor = DatabaseActor::spawn(DatabaseActorArgs {
            campaign_id: campaign_id.clone(),
            conn,
        });
        (actor, campaign_id)
    }

    #[tokio::test]
    async fn ping_returns_pong() {
        let (actor, _) = spawn_with_migrations().await;
        let reply = actor.ask(Ping).await.expect("ask Ping");
        assert_eq!(reply, Pong);
    }

    #[tokio::test]
    async fn patch_with_wizard_complete_writes_metadata() {
        let (actor, _) = spawn_with_migrations().await;
        let result = actor
            .ask(PatchCampaignMetadata {
                name: Some("Curse of Strahd".into()),
                tagline: Some("Gothic horror in Barovia".into()),
                game_system: Some("D&D 5e".into()),
                content_locale: Some("en".into()),
                complete_wizard: true,
            })
            .await
            .expect("patch should succeed");

        assert!(result.wizard_just_completed);
        assert!(result.model.wizard_completed_at.is_some());
        assert_eq!(result.model.name, "Curse of Strahd");
        assert_eq!(
            result.model.tagline.as_deref(),
            Some("Gothic horror in Barovia")
        );
        assert_eq!(result.model.game_system.as_deref(), Some("D&D 5e"));
        assert_eq!(result.model.content_locale.as_deref(), Some("en"));
    }

    #[tokio::test]
    async fn patch_without_wizard_complete_updates_fields_only() {
        let (actor, _) = spawn_with_migrations().await;
        let result = actor
            .ask(PatchCampaignMetadata {
                name: Some("Renamed Campaign".into()),
                tagline: None,
                game_system: None,
                content_locale: None,
                complete_wizard: false,
            })
            .await
            .expect("patch should succeed");

        assert!(!result.wizard_just_completed);
        assert!(result.model.wizard_completed_at.is_none());
        assert_eq!(result.model.name, "Renamed Campaign");
    }

    #[tokio::test]
    async fn double_wizard_complete_returns_already_initialized() {
        let (actor, _) = spawn_with_migrations().await;
        actor
            .ask(PatchCampaignMetadata {
                name: Some("First".into()),
                tagline: None,
                game_system: Some("PF2e".into()),
                content_locale: Some("en".into()),
                complete_wizard: true,
            })
            .await
            .expect("first patch");

        let err = actor
            .ask(PatchCampaignMetadata {
                name: Some("Second".into()),
                tagline: None,
                game_system: Some("Blades".into()),
                content_locale: Some("en".into()),
                complete_wizard: true,
            })
            .await
            .expect_err("second wizard_complete should be rejected");

        assert!(
            matches!(
                err,
                kameo::error::SendError::HandlerError(PatchCampaignError::AlreadyInitialized)
            ),
            "expected AlreadyInitialized, got {err:?}"
        );
    }

    #[tokio::test]
    async fn get_metadata_returns_row() {
        let (actor, _) = spawn_with_migrations().await;
        let meta = actor.ask(GetMetadata).await.expect("metadata should exist");
        assert_eq!(meta.id, campaign_metadata::METADATA_ROW_ID);
        assert_eq!(meta.name, "Untitled");
    }

    #[tokio::test]
    async fn graceful_stop_completes() {
        let (actor, _) = spawn_with_migrations().await;
        actor.stop_gracefully().await.expect("stop_gracefully");
        actor.wait_for_shutdown_with_result(|_| ()).await;
    }
}
