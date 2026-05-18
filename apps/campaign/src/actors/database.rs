//! `DatabaseActor`: single owner of the per-campaign sea-orm write
//! connection.
//!
//! At the time of writing the actor accepts no commands beyond [`Ping`]
//! (used by tests to confirm the actor is alive). It exists so the
//! supervisor can own its lifecycle and `on_stop` ordering. Once the
//! wizard seal handler lands, the command set fills out with
//! `SealWizard`, snapshot writes, suggestion outcomes, and similar;
//! every write to the campaign DB will flow through this actor.
//!
//! The architectural commitment is single-writer: the actor system owns
//! the only [`sea_orm::DatabaseConnection`] for a given campaign, so no
//! HTTP handler can acquire a connection without going through the
//! supervisor.

use familiar_systems_app_shared::id::CampaignId;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use sea_orm::DatabaseConnection;

pub struct DatabaseActor {
    campaign_id: CampaignId,
    #[allow(dead_code)] // Reads/writes land in the next PR (wizard seal).
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

/// Health-check message. Returns `Pong` when the actor's mailbox is alive
/// and being processed. Used by tests; not part of the production wire
/// surface.
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
    use kameo::actor::Spawn;

    async fn open_in_memory() -> DatabaseConnection {
        db::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite open")
    }

    #[tokio::test]
    async fn ping_returns_pong() {
        let conn = open_in_memory().await;
        let actor_ref = DatabaseActor::spawn(DatabaseActorArgs {
            campaign_id: CampaignId::generate(),
            conn,
        });
        let reply = actor_ref.ask(Ping).await.expect("ask Ping");
        assert_eq!(reply, Pong);
    }

    #[tokio::test]
    async fn graceful_stop_completes() {
        let conn = open_in_memory().await;
        let actor_ref = DatabaseActor::spawn(DatabaseActorArgs {
            campaign_id: CampaignId::generate(),
            conn,
        });
        actor_ref.stop_gracefully().await.expect("stop_gracefully");
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
    }
}
