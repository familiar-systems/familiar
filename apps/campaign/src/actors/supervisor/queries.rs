//! Read-side + metadata handlers off the shared reader pool: campaign metadata
//! patch/get, landing-page pointer, and the relationship modals' session/entity
//! pickers. Plain reads, not graph concerns, so they sit on the supervisor.

use std::time::Instant;

use familiar_systems_campaign_shared::id::{PageId, SessionId};
use kameo::message::{Context, Message};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect};

use super::CampaignSupervisor;
use crate::actors::database_writer::{
    DbSetLandingPage, GetMetadata, MetadataError, PatchCampaignError,
    PatchCampaignMetadata as DbPatchCampaign, PatchCampaignResult,
};
use crate::entities::columns::PageKindCol;
use crate::entities::{campaign_metadata, pages, sessions};

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

impl Message<PatchCampaignMetadata> for CampaignSupervisor {
    type Reply = Result<PatchCampaignResult, PatchCampaignError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: PatchCampaignMetadata,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        match db
            .writer()
            .ask(DbPatchCampaign {
                name: msg.name,
                tagline: msg.tagline,
                game_system: msg.game_system,
                content_locale: msg.content_locale,
                complete_wizard: msg.complete_wizard,
            })
            .await
        {
            Ok(result) => Ok(result),
            Err(kameo::error::SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "database actor unavailable during patch");
                Err(PatchCampaignError::ActorUnavailable)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// GetMetadata
// ---------------------------------------------------------------------------

impl Message<GetMetadata> for CampaignSupervisor {
    type Reply = Result<campaign_metadata::Model, MetadataError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        _: GetMetadata,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(db.reader())
            .await?
            .ok_or(MetadataError::NoMetadataRow)
    }
}

// ---------------------------------------------------------------------------
// SetLandingPage
// ---------------------------------------------------------------------------

/// Point `campaign_metadata.home_page_id` at a Page (the campaign's home /
/// landing page). System-set during seeding, never mirrored to the platform
/// (it is a local display preference, unlike the wizard-seal metadata). Kept
/// distinct from `PatchCampaignMetadata` so the wizard path stays clean.
#[derive(Debug, Clone)]
pub struct SetLandingPage {
    pub page_id: PageId,
}

impl Message<SetLandingPage> for CampaignSupervisor {
    type Reply = Result<(), PatchCampaignError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.page_id.0),
    )]
    async fn handle(
        &mut self,
        msg: SetLandingPage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        match db
            .writer()
            .ask(DbSetLandingPage {
                page_id: msg.page_id,
            })
            .await
        {
            Ok(()) => Ok(()),
            Err(kameo::error::SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "database actor unavailable during set landing page");
                Err(PatchCampaignError::ActorUnavailable)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session + entity reads (the relationship modals' pickers)
// ---------------------------------------------------------------------------
//
// Plain reads off the shared reader pool (the `GetMetadata` idiom), not graph
// concerns, so they live on the supervisor rather than the RelationshipGraph.

/// Every session, ascending by ordinal, as `(id, ordinal)` - the as-of pickers'
/// source. The id is durable; ordinals can be renumbered.
#[derive(Debug, Clone, Copy)]
pub struct ListSessions;

impl Message<ListSessions> for CampaignSupervisor {
    type Reply = Result<Vec<(SessionId, i64)>, sea_orm::DbErr>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        _: ListSessions,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        let rows = sessions::Entity::find()
            .order_by_asc(sessions::Column::Ordinal)
            .all(db.reader())
            .await?;
        Ok(rows
            .into_iter()
            .map(|s| (SessionId::from(s.id), s.ordinal))
            .collect())
    }
}

/// Entity-page name matches for the create modal's object typeahead. Substring
/// match on the title; templates are a separate kind, so selecting `Entity`
/// excludes them. Minimal `LIKE`; contract frozen for a later Tantivy /
/// `CampaignVocabulary` swap.
#[derive(Debug, Clone)]
pub struct SearchEntities {
    pub query: String,
    pub limit: u64,
}

impl Message<SearchEntities> for CampaignSupervisor {
    type Reply = Result<Vec<(PageId, String)>, sea_orm::DbErr>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        msg: SearchEntities,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");
        let rows = pages::Entity::find()
            .filter(pages::Column::Kind.eq(PageKindCol::Entity))
            .filter(pages::Column::Name.contains(msg.query.as_str()))
            // FIXME we should not do this because because `%` and `_` are interpreted
            // as wildcards in `LIKE` causing us to fail on like this.
            // When moving ot tantivy for the name server, ensure we have test for this
            // and/or use parameter fuzzing.
            .order_by_asc(pages::Column::Name)
            .limit(msg.limit)
            .all(db.reader())
            .await?;
        Ok(rows
            .into_iter()
            .map(|p| (PageId::from(p.id), p.name))
            .collect())
    }
}
