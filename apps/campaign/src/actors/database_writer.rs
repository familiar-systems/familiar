//! `DatabaseActor`: single owner of the per-campaign sea-orm write
//! connection.
//!
//! Every write to the campaign DB flows through this actor's mailbox.

use chrono::{DateTime, Utc};
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
// InitializeCampaignSetup
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct InitializeCampaignSetup {
    pub name: String,
    pub tagline: Option<String>,
    pub game_system: String,
    pub content_locale: String,
}

#[derive(Debug, thiserror::Error)]
pub enum InitializeCampaignError {
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
pub struct InitializeCampaignResult {
    pub wizard_completed_at: DateTime<Utc>,
}

impl Message<InitializeCampaignSetup> for DatabaseActor {
    type Reply = Result<InitializeCampaignResult, InitializeCampaignError>;

    async fn handle(
        &mut self,
        msg: InitializeCampaignSetup,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let existing = campaign_metadata::Entity::find_by_id(1)
            .one(&self.conn)
            .await?
            .ok_or(InitializeCampaignError::NoMetadataRow)?;

        if existing.wizard_completed_at.is_some() {
            return Err(InitializeCampaignError::AlreadyInitialized);
        }

        let now = Utc::now();
        let mut am: campaign_metadata::ActiveModel = existing.into();
        am.name = Set(msg.name);
        am.tagline = Set(msg.tagline);
        am.game_system = Set(Some(msg.game_system));
        am.content_locale = Set(Some(msg.content_locale));
        am.wizard_completed_at = Set(Some(now));
        am.updated_at = Set(now);
        am.update(&self.conn).await?;

        Ok(InitializeCampaignResult {
            wizard_completed_at: now,
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
        campaign_metadata::Entity::find_by_id(1)
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
            id: Set(1),
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
    async fn initialize_campaign_writes_metadata() {
        let (actor, _) = spawn_with_migrations().await;
        let result = actor
            .ask(InitializeCampaignSetup {
                name: "Curse of Strahd".into(),
                tagline: Some("Gothic horror in Barovia".into()),
                game_system: "D&D 5e".into(),
                content_locale: "en".into(),
            })
            .await
            .expect("initialize should succeed");

        assert!(result.wizard_completed_at <= Utc::now());

        let meta = actor.ask(GetMetadata).await.expect("metadata should exist");
        assert_eq!(meta.name, "Curse of Strahd");
        assert_eq!(meta.tagline.as_deref(), Some("Gothic horror in Barovia"));
        assert_eq!(meta.game_system.as_deref(), Some("D&D 5e"));
        assert_eq!(meta.content_locale.as_deref(), Some("en"));
        assert!(meta.wizard_completed_at.is_some());
    }

    #[tokio::test]
    async fn initialize_campaign_rejects_double_init() {
        let (actor, _) = spawn_with_migrations().await;
        actor
            .ask(InitializeCampaignSetup {
                name: "First".into(),
                tagline: None,
                game_system: "PF2e".into(),
                content_locale: "en".into(),
            })
            .await
            .expect("first initialize");

        let err = actor
            .ask(InitializeCampaignSetup {
                name: "Second".into(),
                tagline: None,
                game_system: "Blades".into(),
                content_locale: "en".into(),
            })
            .await
            .expect_err("second initialize should be rejected");

        assert!(
            matches!(
                err,
                kameo::error::SendError::HandlerError(InitializeCampaignError::AlreadyInitialized)
            ),
            "expected AlreadyInitialized, got {err:?}"
        );
    }

    #[tokio::test]
    async fn get_metadata_returns_row() {
        let (actor, _) = spawn_with_migrations().await;
        let meta = actor.ask(GetMetadata).await.expect("metadata should exist");
        assert_eq!(meta.id, 1);
        assert_eq!(meta.name, "Untitled");
    }

    #[tokio::test]
    async fn graceful_stop_completes() {
        let (actor, _) = spawn_with_migrations().await;
        actor.stop_gracefully().await.expect("stop_gracefully");
        actor.wait_for_shutdown_with_result(|_| ()).await;
    }
}
