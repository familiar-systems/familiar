//! `CampaignSupervisor`: per-campaign orchestrator.
//!
//! Owns the [`CampaignDatabase`] and an idle-eviction clock.
//!
//! Child actors:
//! - [`TocActor`] (CRDT singleton, eager)
//! - [`RelationshipGraph`] (server-authoritative graph singleton, eager)
//! - [`PageActor`] (per-page, lazy-spawned on first `JoinRoom`). Future: AgentConversation, CampaignVocabulary.
//!
//! Storage initialization (checkout, open connection, run migrations,
//! spawn DatabaseWriteActor) runs in `on_start` via
//! [`CampaignDatabase::checkout`]. The registry's `EnsureCampaign`
//! handler awaits the startup result before inserting the supervisor
//! into its map.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::sync::Arc;
use std::time::{Duration, Instant};

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use kameo::actor::{ActorId, ActorRef, Spawn, WeakActorRef};
use kameo::error::{ActorStopReason, SendError};
use kameo::message::{Context, Message};
use kameo::prelude::Actor;

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
    QuerySelect,
};

use familiar_systems_campaign_shared::id::{ClientId, PageId, SessionId};
use familiar_systems_campaign_shared::loro::toc::TocPageKind;
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::relationship::RelationshipView;
use familiar_systems_campaign_shared::status::Status;
use tokio::sync::{mpsc, oneshot};

use crate::actors::database_writer::{
    CreatedSession, DbSetLandingPage, GetMetadata, MetadataError, PatchCampaignError,
    PatchCampaignMetadata as DbPatchCampaign, PatchCampaignResult,
};
use crate::actors::page::{PageActor, PageActorArgs, PageInit};
use crate::actors::relationship_graph::{
    CreateRelationship, CreateRelationshipError, DeleteRelationship, DeleteRelationshipError,
    KnownPredicatePairs, PatchRelationship, PatchRelationshipError, RelationshipGraph,
    RelationshipGraphArgs, RelationshipsForPage,
};
use crate::actors::toc::{AddPageNode, ResolvePageNode, TocActor, TocActorArgs};
use crate::clients::platform_internal::PlatformInternalClient;
use crate::domain::crdt::room_actor;
use crate::domain::page::DocumentPageKind;
use crate::domain::relationship::PredicatePair;
use crate::entities::columns::{PageIdCol, PageKindCol};
use crate::entities::{campaign_metadata, pages, sessions};
use crate::error::InitError;
use crate::persistence::{CampaignDatabase, CampaignStore};

// TODO: replace `Option<CampaignDatabase>` with a `SupervisorState` enum
// (Starting / Restoring / Ready / Draining) per the actor domain design doc.
// The current Option works while checkout is synchronous and there are no
// room actors, but the state machine is needed for heartbeat phase reporting
// and room-join gating once WebSocket support lands.
pub struct CampaignSupervisor {
    campaign_id: CampaignId,
    store: Arc<dyn CampaignStore>,
    db: Option<CampaignDatabase>,
    toc: ActorRef<TocActor>,
    relationship_graph: ActorRef<RelationshipGraph>,
    pages: HashMap<PageId, ActorRef<PageActor>>,
    last_activity: Instant,
    idle_timeout: Duration,
    stop_cause: Option<StopCause>,
    platform_client: Option<PlatformInternalClient>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    Idle,
    Drain,
    RegistryFallback,
    PlatformRelease,
}

impl StopCause {
    fn as_str(self) -> &'static str {
        match self {
            StopCause::Idle => "idle",
            StopCause::Drain => "drain",
            StopCause::RegistryFallback => "registry_fallback",
            StopCause::PlatformRelease => "platform_release",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SetStopCause(pub StopCause);

impl Message<SetStopCause> for CampaignSupervisor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: SetStopCause,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.stop_cause.get_or_insert(msg.0);
    }
}

pub struct CampaignSupervisorArgs {
    pub campaign_id: CampaignId,
    pub owner_user_id: Option<UserId>,
    pub store: Arc<dyn CampaignStore>,
    pub idle_timeout: Duration,
    pub eviction_check_interval: Duration,
    pub platform_client: Option<PlatformInternalClient>,
}

impl Actor for CampaignSupervisor {
    type Args = CampaignSupervisorArgs;
    type Error = InitError;

    // TODO: move checkout to a background task per the actor domain design
    // doc's startup lifecycle. Synchronous on_start blocks the supervisor's
    // mailbox during checkout, which is fine for LocalCampaignStore (sub-ms)
    // but will block for seconds once S3 checkout downloads the .db file.
    // The design doc's pattern: spawn checkout as a tokio task, transition
    // through Starting -> Restoring -> Ready via completion messages.
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %args.campaign_id.0),
    )]
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, InitError> {
        let (db, is_new) = CampaignDatabase::checkout(
            args.store.as_ref(),
            &args.campaign_id,
            args.owner_user_id.as_ref(),
        )
        .await?;

        let toc = TocActor::spawn(TocActorArgs {
            campaign_id: args.campaign_id.clone(),
            db_reader: db.reader().clone(),
            db_writer: db.writer().clone(),
            debounce_duration: Duration::from_secs(2),
        });

        let relationship_graph = RelationshipGraph::spawn(RelationshipGraphArgs {
            campaign_id: args.campaign_id.clone(),
            db_reader: db.reader().clone(),
            db_writer: db.writer().clone(),
        });

        tracing::info!("campaign ready");

        let timer_ref = actor_ref.clone();
        let interval = args.eviction_check_interval;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            tick.tick().await;
            loop {
                tick.tick().await;
                if timer_ref.tell(IdleCheck).await.is_err() {
                    break;
                }
            }
        });

        // Seed the campaign's home page exactly once, on first-ever checkout.
        // Spawned (not inline) because on_start has not returned yet: the mailbox
        // is not draining, so a self-`ask` would deadlock. This runs after
        // on_start completes, going through the same CreatePage path the GM's
        // future "new page" button uses. Best-effort: a failure is cosmetic, and
        // the orphan path re-surfaces any created Page at the ToC root on the
        // next checkout.
        if is_new {
            let seed_ref = actor_ref.clone();
            tokio::spawn(async move {
                // The home page's sections (and the editable empty paragraph each
                // is seeded with, stable-id from genesis) come from its kind inside
                // the PageActor; this caller names the page and never enumerates
                // sections.
                match seed_ref
                    .ask(CreatePage {
                        name: "Campaign Base Camp".to_string(),
                        status: Some(Status::Known),
                        parent: None,
                        kind: DocumentPageKind::Entity,
                    })
                    .await
                {
                    // What happens if this fails to land correctly? Nothing, really.
                    // Users can still always make their own home pages and other new pages.
                    // Well, at least when setting a new home page lands - that's still a TODO!
                    Ok(page) => {
                        let page_id = PageId::from(page.id);
                        if let Err(e) = seed_ref.ask(SetLandingPage { page_id }).await {
                            tracing::warn!(error = %e, "failed to record campaign home page pointer");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to seed campaign home page");
                    }
                }
            });
        }

        Ok(Self {
            campaign_id: args.campaign_id,
            store: args.store,
            db: Some(db),
            toc,
            relationship_graph,
            pages: HashMap::new(),
            last_activity: Instant::now(),
            idle_timeout: args.idle_timeout,
            stop_cause: None,
            platform_client: args.platform_client,
        })
    }

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        let cause = match (self.stop_cause, &reason) {
            (Some(c), _) => c.as_str(),
            (None, ActorStopReason::Normal | ActorStopReason::SupervisorRestart) => "signal",
            (None, ActorStopReason::Killed) => "killed",
            (None, ActorStopReason::Panicked(_)) => "crash",
            (None, ActorStopReason::LinkDied { .. }) => "link_died",
        };
        tracing::info!(cause, "draining supervisor");
        let started = Instant::now();

        // Stop all PageActors, then TocActor, before DatabaseWriteActor
        // so any pending writebacks reach the writer before it drains.
        for (page_id, actor) in self.pages.drain() {
            if let Err(e) = actor.stop_gracefully().await {
                tracing::warn!(page_id = %page_id.0, error = ?e, "page actor already stopped during drain");
            }
            actor.wait_for_shutdown_with_result(|_| ()).await;
        }
        if let Err(e) = self.toc.stop_gracefully().await {
            tracing::warn!(error = ?e, "toc actor already stopped during drain");
        }
        self.toc.wait_for_shutdown_with_result(|_| ()).await;

        // The relationship graph writes through synchronously (no dirty state to
        // flush), but stop it before the DB connection closes for clean ordering.
        if let Err(e) = self.relationship_graph.stop_gracefully().await {
            tracing::warn!(error = ?e, "relationship graph already stopped during drain");
        }
        self.relationship_graph
            .wait_for_shutdown_with_result(|_| ())
            .await;

        if let Some(db) = self.db.take()
            && let Err(e) = db.release(self.store.as_ref(), &self.campaign_id).await
        {
            tracing::error!(
                error = %e,
                "storage release failed; campaign data may not be fully persisted"
            );
        }

        if matches!(self.stop_cause, Some(StopCause::Idle))
            && let Some(ref client) = self.platform_client
        {
            let campaign_id = self.campaign_id.0.0.clone();
            let client = client.clone();
            tokio::spawn(async move {
                if let Err(e) = client.release_lease(&campaign_id).await {
                    tracing::warn!(
                        campaign_id = %campaign_id,
                        error = %e,
                        "failed to notify platform of idle release"
                    );
                }
            });
        }

        tracing::info!(
            drain_elapsed_ms = started.elapsed().as_millis() as u64,
            "supervisor stopped"
        );
        Ok(())
    }

    // The supervisor is `link`ed to two kinds of siblings (kameo links are
    // bidirectional): its parent `CampaignRegistry` and each child `PageActor`
    // it spawns. This handler fires for both, so it must tell them apart and
    // react differently. Children are linked *after* insertion into `pages`, so
    // a dead sibling whose id is still in the map is necessarily a child.
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn on_link_died(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        id: ActorId,
        reason: ActorStopReason,
    ) -> Result<ControlFlow<ActorStopReason>, Self::Error> {
        // A child PageActor died (idle self-evict, drain, or panic). Prune its
        // entry and keep running: a single room going away (or even crashing)
        // must not take down the whole campaign and every other room. Each Page
        // is in `pages` when its one-shot link_died fires (linked after insert;
        // `on_stop`'s drain is terminal), so a hit here means "child".
        let before = self.pages.len();
        self.pages.retain(|_, actor| actor.id() != id);
        if self.pages.len() != before {
            tracing::debug!(
                ?reason,
                page_count = self.pages.len(),
                "page actor removed from supervisor via link_died"
            );
            return Ok(ControlFlow::Continue(()));
        }

        // No map entry matched: the only other sibling is the registry parent.
        // Preserve kameo's default propagation so an abnormal registry death
        // still tears the supervisor down (-> `on_stop` flush + DB release),
        // while a normal one is benign.
        match &reason {
            ActorStopReason::Normal | ActorStopReason::SupervisorRestart => {
                Ok(ControlFlow::Continue(()))
            }
            _ => Ok(ControlFlow::Break(ActorStopReason::LinkDied {
                id,
                reason: Box::new(reason),
            })),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct IdleCheck;

impl Message<IdleCheck> for CampaignSupervisor {
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(&mut self, _: IdleCheck, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        if self.stop_cause.is_some() {
            return;
        }
        let elapsed = self.last_activity.elapsed();
        if elapsed >= self.idle_timeout {
            tracing::info!(
                idle_seconds = elapsed.as_secs(),
                "idle timeout reached, beginning eviction"
            );
            self.stop_cause = Some(StopCause::Idle);
            ctx.stop();
        } else {
            tracing::trace!(idle_seconds = elapsed.as_secs(), "idle check, still active");
        }
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
// CreatePage
// ---------------------------------------------------------------------------

/// Create a new **document page** (an `Entity` or `Template`) in this campaign.
/// The supervisor validates placement, spawns the owning `PageActor` in genesis
/// mode (which persists the Page's own birth row), registers it, and adds its
/// node to the live ToC. Replies with the persisted `pages` row for the HTTP
/// response.
///
/// `kind` is a [`DocumentPageKind`], so a `Session` is **unrepresentable** here:
/// it mints a temporal row and has its own [`CreateSession`] message. A future
/// Skill / Memory kind, being document-shaped, joins that sum and routes here.
#[derive(Debug, Clone)]
pub struct CreatePage {
    pub name: String,
    pub status: Option<Status>,
    /// Parent Page to nest under in the ToC. `None` => ToC root.
    pub parent: Option<PageId>,
    /// Which document-page kind to genesis (`Entity` or `Template`).
    pub kind: DocumentPageKind,
}

#[derive(Debug, thiserror::Error)]
pub enum CreatePageError {
    #[error("parent page not found in toc")]
    ParentNotFound,
    #[error("page name must not be empty")]
    EmptyName,
    #[error("a {0:?} page named {1:?} already exists")]
    NameTaken(PageKind, String),
    #[error("page genesis failed")]
    Genesis,
    #[error("a child actor was unavailable")]
    ActorUnavailable,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

/// Whether a page of `kind` already carries `name` (exact, case-sensitive match;
/// the caller has already trimmed). Names are unique **per kind**, so an entity
/// and a session may share "The Fall of Perth", but two sessions may not.
///
/// Enforced here at the application layer rather than with a DB constraint: a
/// composite `(kind, name)` unique index is not expressible on the sea-orm entity
/// (`Schema::create_table_from_entity` only sees single-column `unique`), so it
/// would break the schema-drift test with no entity-side mirror. This
/// read-then-insert is race-free under the single-writer + serialized-supervisor
/// invariant - the same property the ordinal `MAX + 1` relies on.
async fn name_taken_for_kind(
    db_reader: &DatabaseConnection,
    kind: PageKind,
    name: &str,
) -> Result<bool, sea_orm::DbErr> {
    let existing = pages::Entity::find()
        .filter(pages::Column::Kind.eq(PageKindCol::from(kind)))
        .filter(pages::Column::Name.eq(name))
        .one(db_reader)
        .await?;
    Ok(existing.is_some())
}

impl Message<CreatePage> for CampaignSupervisor {
    type Reply = Result<pages::Model, CreatePageError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: CreatePage,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();

        let name = msg.name.trim().to_string();
        if name.is_empty() {
            return Err(CreatePageError::EmptyName);
        }

        // Validate placement before any write.
        // Bad parent fails cleanly with nothing persisted.
        if let Some(parent) = &msg.parent {
            match self.toc.ask(ResolvePageNode(parent.clone())).await {
                Ok(Some(_)) => {}
                Ok(None) => return Err(CreatePageError::ParentNotFound),
                Err(e) => {
                    tracing::error!(error = %e, "toc unavailable while resolving parent");
                    return Err(CreatePageError::ActorUnavailable);
                }
            }
        }

        let status = msg.status.unwrap_or(Status::GmOnly);
        let kind = PageKind::from(msg.kind);

        let (db_reader, db_writer) = {
            let db = self
                .db
                .as_ref()
                .expect("db must be Some while actor is running");
            (db.reader().clone(), db.writer().clone())
        };

        // Names are unique per kind (see `name_taken_for_kind`).
        if name_taken_for_kind(&db_reader, kind, &name).await? {
            return Err(CreatePageError::NameTaken(kind, name));
        }

        // `DocumentPageKind::toc_page_kind` owns the kind -> ToC-node map, so this
        // path no longer restates it; document pages carry no ordinal.
        let toc_page_kind = msg.kind.toc_page_kind();
        // The owning actor threads its committed `pages::Model` back here instead
        // of us re-reading it from the reader pool after genesis.
        let (reply_tx, reply_rx) = oneshot::channel();
        let init = PageInit::NewDocumentPage {
            name: name.clone(),
            kind: msg.kind,
            status,
            reply: reply_tx,
        };

        let page_id = PageId::generate();

        // Spawn the owning actor in genesis mode; it persists its own birth row
        // through the single-writer. Nothing writes a Page's rows around it.
        let actor = PageActor::spawn(PageActorArgs {
            campaign_id: self.campaign_id.clone(),
            page_id: page_id.clone(),
            db_reader: db_reader.clone(),
            db_writer,
            toc: self.toc.clone(),
            init,
            debounce_duration: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(30),
        });
        actor.wait_for_startup().await;
        if !actor.is_alive() {
            tracing::error!("page actor died during genesis");
            return Err(CreatePageError::Genesis);
        }
        self.pages.insert(page_id.clone(), actor.clone());
        // Link after insert so `on_link_died` prunes this entry when the actor
        // self-evicts on idle (see the handler for the after-insert rationale).
        ctx.actor_ref().clone().link(&actor).await;

        // Place it in the live ToC. Best-effort: a failure here leaves a valid
        // Page that `restore_toc` re-surfaces at the root on the next checkout.
        if let Err(e) = self
            .toc
            .ask(AddPageNode {
                page_id: page_id.clone(),
                title: name,
                // Entity or Template here (Session has its own path).
                page_kind: toc_page_kind,
                visibility: status,
                parent: msg.parent.clone(),
            })
            .await
        {
            tracing::error!(
                error = %e,
                "failed to add toc node for new page; it will self-heal on next checkout"
            );
        }

        // The owning actor threaded its committed row back through the genesis
        // oneshot; return it directly, no read-after-write. The actor is alive
        // (checked above), so the send fired; a recv error is defensive only.
        reply_rx.await.map_err(|_| CreatePageError::Genesis)
    }
}

// ---------------------------------------------------------------------------
// CreateSession
// ---------------------------------------------------------------------------

/// Create a new session: its Session page and the temporal `sessions` row,
/// minted together in one genesis transaction. The supervisor validates
/// placement, spawns the owning `PageActor` in session-genesis mode (which
/// drives the atomic `DbCreateSession`), registers it, and adds its node to the
/// live ToC. Replies with the persisted page + session rows.
///
/// This is the reactive-shell orchestration of "mint a session": the effectful
/// domain write composes into the page's genesis txn; the supervisor sequences
/// genesis, registration, and ToC placement around it.
#[derive(Debug, Clone)]
pub struct CreateSession {
    /// The session's name. Required and non-blank, and unique among sessions,
    /// like every other page kind; the client renders `Session {ordinal}: {name}`.
    pub name: String,
    pub status: Option<Status>,
    /// Parent to nest under in the ToC. `None` => ToC root.
    pub parent: Option<PageId>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateSessionError {
    #[error("parent page not found in toc")]
    ParentNotFound,
    #[error("session name must not be empty")]
    EmptyName,
    #[error("a {0:?} page named {1:?} already exists")]
    NameTaken(PageKind, String),
    #[error("session genesis failed")]
    Genesis,
    #[error("a child actor was unavailable")]
    ActorUnavailable,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

impl Message<CreateSession> for CampaignSupervisor {
    type Reply = Result<CreatedSession, CreateSessionError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: CreateSession,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();

        // A session is named like every other page kind: required and non-blank.
        let name = msg.name.trim().to_string();
        if name.is_empty() {
            return Err(CreateSessionError::EmptyName);
        }

        // Validate placement before any write. Bad parent fails cleanly.
        if let Some(parent) = &msg.parent {
            match self.toc.ask(ResolvePageNode(parent.clone())).await {
                Ok(Some(_)) => {}
                Ok(None) => return Err(CreateSessionError::ParentNotFound),
                Err(e) => {
                    tracing::error!(error = %e, "toc unavailable while resolving parent");
                    return Err(CreateSessionError::ActorUnavailable);
                }
            }
        }

        let status = msg.status.unwrap_or(Status::GmOnly);

        let (db_reader, db_writer) = {
            let db = self
                .db
                .as_ref()
                .expect("db must be Some while actor is running");
            (db.reader().clone(), db.writer().clone())
        };

        // Names are unique per kind, sessions included (see `name_taken_for_kind`).
        if name_taken_for_kind(&db_reader, PageKind::Session, &name).await? {
            return Err(CreateSessionError::NameTaken(PageKind::Session, name));
        }

        let page_id = PageId::generate();

        // The owning actor threads both committed rows (page + temporal session)
        // back here instead of us re-reading them from the reader pool.
        let (reply_tx, reply_rx) = oneshot::channel();

        // Spawn the owning actor in session-genesis mode; it persists the page,
        // its blocks, and the temporal row atomically via `DbCreateSession`.
        let actor = PageActor::spawn(PageActorArgs {
            campaign_id: self.campaign_id.clone(),
            page_id: page_id.clone(),
            db_reader: db_reader.clone(),
            db_writer,
            toc: self.toc.clone(),
            init: PageInit::NewSession {
                name: name.clone(),
                status,
                reply: reply_tx,
            },
            debounce_duration: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(30),
        });
        actor.wait_for_startup().await;
        if !actor.is_alive() {
            tracing::error!("session page actor died during genesis");
            return Err(CreateSessionError::Genesis);
        }
        self.pages.insert(page_id.clone(), actor.clone());
        ctx.actor_ref().clone().link(&actor).await;

        // The owning actor threaded both committed rows back through the genesis
        // oneshot; no read-after-write. The ToC node below needs the genesis-
        // assigned `ordinal`, which rides along on `created.session`. Alive
        // (checked above) implies the send fired; a recv error is defensive.
        let created = reply_rx.await.map_err(|_| CreateSessionError::Genesis)?;

        // Place it in the live ToC, carrying the kind and ordinal so clients can
        // render "Session {ordinal}". Best-effort: a failure leaves a valid Page
        // that `restore_toc` re-surfaces (with its ordinal) on the next checkout.
        if let Err(e) = self
            .toc
            .ask(AddPageNode {
                page_id: page_id.clone(),
                title: name,
                page_kind: TocPageKind::Session {
                    ordinal: created.session.ordinal,
                },
                visibility: status,
                parent: msg.parent.clone(),
            })
            .await
        {
            tracing::error!(
                error = %e,
                "failed to add toc node for new session; it will self-heal on next checkout"
            );
        }

        Ok(created)
    }
}

// ---------------------------------------------------------------------------
// RoomHandle + JoinRoom (WebSocket room dispatch)
// ---------------------------------------------------------------------------

/// Handle to a room actor, held in the WebSocket connection's local routing
/// table. Enum (not trait object) because kameo `ActorRef<A>` is generic
/// over the concrete actor type.
#[derive(Clone)]
pub enum RoomHandle {
    Toc(ActorRef<TocActor>),
    Page(ActorRef<PageActor>),
}

impl RoomHandle {
    pub async fn join(
        &self,
        client: ClientId,
        tx: mpsc::UnboundedSender<Vec<u8>>,
        role: CampaignRole,
    ) -> Result<room_actor::JoinResponse, room_actor::JoinError> {
        let msg = room_actor::ClientJoin { client, tx, role };
        match self {
            RoomHandle::Toc(actor) => match actor.ask(msg).await {
                Ok(response) => Ok(response),
                Err(kameo::error::SendError::HandlerError(e)) => Err(e),
                Err(e) => Err(room_actor::JoinError::Internal(e.to_string())),
            },
            RoomHandle::Page(actor) => match actor.ask(msg).await {
                Ok(response) => Ok(response),
                Err(kameo::error::SendError::HandlerError(e)) => Err(e),
                Err(e) => Err(room_actor::JoinError::Internal(e.to_string())),
            },
        }
    }

    pub async fn update(
        &self,
        client: ClientId,
        updates: Vec<Vec<u8>>,
    ) -> Result<room_actor::AckPayload, room_actor::UpdateError> {
        let msg = room_actor::ClientUpdate { client, updates };
        // Map kameo's transport-layer `SendError` to typed `UpdateError`
        // variants by matching the structured enum, not its Display text.
        // `ActorStopped`/`ActorNotRunning` mean the room actor is gone (a
        // self-evicted room is the common case); `MailboxFull`/`Timeout` mean
        // it is alive but overloaded. Mirrors the idiom in `actors/persist.rs`.
        match self {
            RoomHandle::Toc(actor) => match actor.ask(msg).await {
                Ok(ack) => Ok(ack),
                Err(SendError::HandlerError(e)) => Err(e),
                Err(SendError::ActorNotRunning(_) | SendError::ActorStopped) => {
                    Err(room_actor::UpdateError::RoomGone)
                }
                Err(SendError::MailboxFull(_) | SendError::Timeout(_)) => {
                    Err(room_actor::UpdateError::Busy)
                }
            },
            RoomHandle::Page(actor) => match actor.ask(msg).await {
                Ok(ack) => Ok(ack),
                Err(SendError::HandlerError(e)) => Err(e),
                Err(SendError::ActorNotRunning(_) | SendError::ActorStopped) => {
                    Err(room_actor::UpdateError::RoomGone)
                }
                Err(SendError::MailboxFull(_) | SendError::Timeout(_)) => {
                    Err(room_actor::UpdateError::Busy)
                }
            },
        }
    }

    pub async fn leave(&self, client: ClientId) {
        let msg = room_actor::ClientLeave { client };
        match self {
            RoomHandle::Toc(actor) => {
                let _ = actor.tell(msg).await;
            }
            RoomHandle::Page(actor) => {
                let _ = actor.tell(msg).await;
            }
        }
    }
}

pub struct JoinRoom {
    pub room_id: String,
}

#[derive(Debug, thiserror::Error)]
pub enum JoinRoomError {
    #[error("unknown room: {0}")]
    UnknownRoom(String),
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

impl CampaignSupervisor {
    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %page_id.0),
    )]
    async fn ensure_page_actor(
        &mut self,
        page_id: PageId,
        supervisor_ref: ActorRef<Self>,
    ) -> Result<ActorRef<PageActor>, JoinRoomError> {
        if let Some(actor) = self.pages.get(&page_id)
            && actor.is_alive()
        {
            return Ok(actor.clone());
        }

        let db = self
            .db
            .as_ref()
            .expect("db must be Some while actor is running");

        let exists = pages::Entity::find()
            .filter(pages::Column::Id.eq(PageIdCol::from(page_id.clone())))
            .count(db.reader())
            .await?
            > 0;

        if !exists {
            return Err(JoinRoomError::UnknownRoom(format!("page:{}", page_id.0)));
        }

        let actor = PageActor::spawn(PageActorArgs {
            campaign_id: self.campaign_id.clone(),
            page_id: page_id.clone(),
            db_reader: db.reader().clone(),
            db_writer: db.writer().clone(),
            toc: self.toc.clone(),
            init: PageInit::Restore,
            debounce_duration: Duration::from_secs(2),
            idle_timeout: Duration::from_secs(30),
        });

        self.pages.insert(page_id, actor.clone());
        // Link after insert so `on_link_died` prunes this entry when the actor
        // self-evicts on idle (see the handler for the after-insert rationale).
        supervisor_ref.link(&actor).await;
        Ok(actor)
    }
}

impl Message<JoinRoom> for CampaignSupervisor {
    type Reply = Result<RoomHandle, JoinRoomError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, room_id = %msg.room_id),
    )]
    async fn handle(&mut self, msg: JoinRoom, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.last_activity = Instant::now();
        match msg.room_id.as_str() {
            "toc" => Ok(RoomHandle::Toc(self.toc.clone())),
            _ if msg.room_id.starts_with("page:") => {
                let id_str = &msg.room_id["page:".len()..];
                let ulid = ulid::Ulid::from_string(id_str)
                    .map_err(|_| JoinRoomError::UnknownRoom(msg.room_id.clone()))?;
                let page_id = PageId::from(ulid);
                let actor = self
                    .ensure_page_actor(page_id, ctx.actor_ref().clone())
                    .await?;
                Ok(RoomHandle::Page(actor))
            }
            _ => Err(JoinRoomError::UnknownRoom(msg.room_id)),
        }
    }
}

// ---------------------------------------------------------------------------
// Ping (health check / test)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ping;

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub struct Pong;

impl Message<Ping> for CampaignSupervisor {
    type Reply = Pong;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.last_activity = Instant::now();
        Pong
    }
}

// ---------------------------------------------------------------------------
// Relationships (forwarded to the RelationshipGraph singleton)
// ---------------------------------------------------------------------------
//
// The supervisor is the only front door; the `RelationshipGraph` ref stays private.
// Each handler forwards and folds a transport failure (the actor unreachable) into
// the message's own error channel, matching the `GetMetadata`/`CreatePage` idiom.

impl Message<RelationshipsForPage> for CampaignSupervisor {
    type Reply = Result<Vec<RelationshipView>, sea_orm::DbErr>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        msg: RelationshipsForPage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        match self.relationship_graph.ask(msg).await {
            Ok(views) => Ok(views),
            Err(SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "relationship graph unavailable reading relationships");
                Err(sea_orm::DbErr::Custom(
                    "relationship graph unavailable".into(),
                ))
            }
        }
    }
}

impl Message<CreateRelationship> for CampaignSupervisor {
    type Reply = Result<RelationshipView, CreateRelationshipError>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        msg: CreateRelationship,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        match self.relationship_graph.ask(msg).await {
            Ok(view) => Ok(view),
            Err(SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "relationship graph unavailable creating relationship");
                Err(CreateRelationshipError::ActorUnavailable)
            }
        }
    }
}

impl Message<PatchRelationship> for CampaignSupervisor {
    type Reply = Result<(), PatchRelationshipError>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        msg: PatchRelationship,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        match self.relationship_graph.ask(msg).await {
            Ok(()) => Ok(()),
            Err(SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "relationship graph unavailable patching relationship");
                Err(PatchRelationshipError::ActorUnavailable)
            }
        }
    }
}

impl Message<DeleteRelationship> for CampaignSupervisor {
    type Reply = Result<(), DeleteRelationshipError>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        msg: DeleteRelationship,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        match self.relationship_graph.ask(msg).await {
            Ok(()) => Ok(()),
            Err(SendError::HandlerError(e)) => Err(e),
            Err(e) => {
                tracing::error!(error = %e, "relationship graph unavailable deleting relationship");
                Err(DeleteRelationshipError::ActorUnavailable)
            }
        }
    }
}

impl Message<KnownPredicatePairs> for CampaignSupervisor {
    // The actor's reply is infallible; the supervisor wraps it so an unreachable
    // actor (transport failure) is representable rather than panicking.
    type Reply = Result<Vec<PredicatePair>, sea_orm::DbErr>;

    #[tracing::instrument(skip_all, fields(campaign_id = %self.campaign_id.0))]
    async fn handle(
        &mut self,
        _: KnownPredicatePairs,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        match self.relationship_graph.ask(KnownPredicatePairs).await {
            Ok(pairs) => Ok(pairs),
            Err(e) => {
                tracing::error!(error = %e, "relationship graph unavailable reading predicates");
                Err(sea_orm::DbErr::Custom(
                    "relationship graph unavailable".into(),
                ))
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

#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct GetStopCause;

#[cfg(test)]
impl Message<GetStopCause> for CampaignSupervisor {
    type Reply = Option<StopCause>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        _: GetStopCause,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.stop_cause
    }
}

/// Test-only probe for the private `pages` map: is this Page still tracked?
/// Lets eviction tests assert pruning without exposing the map.
#[cfg(test)]
#[derive(Debug, Clone)]
pub struct ContainsPage(pub PageId);

#[cfg(test)]
impl Message<ContainsPage> for CampaignSupervisor {
    type Reply = bool;

    async fn handle(
        &mut self,
        ContainsPage(id): ContainsPage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.pages.contains_key(&id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::register_sqlite_vec;
    use crate::persistence::LocalCampaignStore;
    use familiar_systems_campaign_shared::id::BlockId;
    use familiar_systems_campaign_shared::page_kind::PageKind;
    use kameo::actor::Spawn;
    use tempfile::TempDir;

    fn ensure_vec0() {
        register_sqlite_vec();
    }

    fn store_in(dir: &std::path::Path) -> Arc<dyn CampaignStore> {
        Arc::new(LocalCampaignStore::new(dir.to_path_buf()))
    }

    fn fast_args(
        campaign_id: CampaignId,
        store: Arc<dyn CampaignStore>,
        idle_ms: u64,
        check_ms: u64,
    ) -> CampaignSupervisorArgs {
        CampaignSupervisorArgs {
            campaign_id,
            owner_user_id: Some(UserId::generate()),
            store,
            idle_timeout: Duration::from_millis(idle_ms),
            eviction_check_interval: Duration::from_millis(check_ms),
            platform_client: None,
        }
    }

    #[tokio::test]
    async fn checkout_creates_db_file() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let campaign_id = CampaignId::generate();
        let args = fast_args(campaign_id.clone(), store, 60_000, 60_000);
        let actor_ref = CampaignSupervisor::spawn(args);
        actor_ref.wait_for_startup().await;

        assert!(tmp.path().join(format!("{}.db", campaign_id.0)).exists());

        actor_ref.stop_gracefully().await.unwrap();
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn ping_returns_pong_and_bumps_activity() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
        let actor_ref = CampaignSupervisor::spawn(args);
        actor_ref.wait_for_startup().await;

        let reply = actor_ref.ask(Ping).await.unwrap();
        assert_eq!(reply, Pong);

        actor_ref.stop_gracefully().await.unwrap();
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
    }

    /// Proves the kameo `SendError` -> `UpdateError` mapping at the
    /// `RoomHandle::update` seam: a stopped room actor yields `RoomGone`, not a
    /// flattened error string. This is the boundary the `classify_update_error`
    /// unit test cannot reach, and the exact case the old substring match
    /// silently misread (kameo's Display is `"actor stopped"`, never the
    /// `"ActorStopped"` Debug casing the connection layer was matching).
    #[tokio::test]
    async fn update_to_stopped_room_actor_is_room_gone() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
        let supervisor = CampaignSupervisor::spawn(args);
        supervisor.wait_for_startup().await;

        let handle = supervisor
            .ask(JoinRoom {
                room_id: "toc".to_string(),
            })
            .await
            .unwrap();

        // Stop the underlying ToC actor so the next send fails at the transport
        // layer (ActorStopped/ActorNotRunning) rather than in the handler.
        let RoomHandle::Toc(ref toc) = handle else {
            panic!("expected a ToC room handle");
        };
        toc.stop_gracefully().await.unwrap();
        toc.wait_for_shutdown_with_result(|_| ()).await;

        let err = handle
            .update(ClientId::new(1), vec![vec![0u8]])
            .await
            .expect_err("update to a stopped actor must fail");
        assert!(
            matches!(err, room_actor::UpdateError::RoomGone),
            "expected RoomGone, got {err:?}",
        );

        supervisor.stop_gracefully().await.unwrap();
        supervisor.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn set_stop_cause_is_first_writer_wins() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
        let actor_ref = CampaignSupervisor::spawn(args);
        actor_ref.wait_for_startup().await;

        actor_ref.tell(SetStopCause(StopCause::Idle)).await.unwrap();
        actor_ref
            .tell(SetStopCause(StopCause::Drain))
            .await
            .unwrap();
        let cause = actor_ref.ask(GetStopCause).await.unwrap();
        assert_eq!(cause, Some(StopCause::Idle));

        actor_ref.stop_gracefully().await.unwrap();
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn idle_supervisor_self_evicts() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 30, 20);
        let actor_ref = CampaignSupervisor::spawn(args);
        actor_ref.wait_for_startup().await;

        tokio::time::sleep(Duration::from_millis(200)).await;
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
        assert!(actor_ref.ask(Ping).await.is_err());
    }

    /// A PageActor that stops (idle self-eviction, or any stop) must be pruned
    /// from the supervisor's `pages` map via the `link` + `on_link_died` edge,
    /// not left as a dead `ActorRef` until the next join of the same id.
    /// Drives the terminal effect directly with `stop_gracefully` (a `Normal`
    /// stop, same as `IdleEvict -> ctx.stop`) so the test is deterministic and
    /// needs no real idle wait.
    #[tokio::test]
    async fn evicted_page_actor_is_pruned_from_map() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
        let supervisor = CampaignSupervisor::spawn(args);
        supervisor.wait_for_startup().await;

        // Create a Page: it is inserted into `pages` and linked.
        let model = supervisor
            .ask(CreatePage {
                name: "Ephemeral".to_string(),
                status: Some(Status::GmOnly),
                parent: None,
                kind: DocumentPageKind::Entity,
            })
            .await
            .unwrap();
        let page_id = PageId::from(model.id.clone());
        assert!(
            supervisor.ask(ContainsPage(page_id.clone())).await.unwrap(),
            "newly created page should be tracked",
        );

        // Join to obtain the live actor ref (returns the same in-map actor),
        // then stop it.
        let handle = supervisor
            .ask(JoinRoom {
                room_id: format!("page:{}", page_id.0),
            })
            .await
            .unwrap();
        let RoomHandle::Page(actor) = handle else {
            panic!("expected a Page room handle");
        };
        actor.stop_gracefully().await.unwrap();
        actor.wait_for_shutdown_with_result(|_| ()).await;

        // link_died is delivered after the actor terminates; poll until pruned.
        let mut pruned = false;
        for _ in 0..50 {
            if !supervisor.ask(ContainsPage(page_id.clone())).await.unwrap() {
                pruned = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            pruned,
            "dead page actor should be pruned from the supervisor map"
        );

        supervisor.stop_gracefully().await.unwrap();
        supervisor.wait_for_shutdown_with_result(|_| ()).await;
    }

    /// Poll the campaign's SQLite file (the seed runs in a spawned task after
    /// `on_start`, so it lands asynchronously) until the home base exists.
    /// Asserts exactly one Page named "Campaign Base Camp" (status `Known`)
    /// with `home_page_id` pointing at it, and returns its id.
    async fn poll_until_seeded(db_path: &std::path::Path) -> PageId {
        for _ in 0..200 {
            let conn = crate::db::connect_readonly(db_path)
                .await
                .expect("open readonly");
            let pages = pages::Entity::find().all(&conn).await.expect("query pages");
            let meta = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
                .one(&conn)
                .await
                .expect("query metadata")
                .expect("metadata row exists");
            // Require both writes (the Page row, then the pointer) so we never
            // observe the brief window between CreatePage and SetLandingPage.
            if let (Some(page), Some(home)) = (pages.first(), meta.home_page_id.clone()) {
                assert_eq!(pages.len(), 1, "exactly one Page seeded");
                assert_eq!(page.name, "Campaign Base Camp");
                assert_eq!(Status::from(page.status), Status::Known);
                let page_id = PageId::from(page.id.clone());
                assert_eq!(
                    PageId::from(home),
                    page_id,
                    "home_page_id points at the base camp"
                );

                // The seed must give the home page one block per declared section
                // (preamble + body), each a paragraph whose row id equals the ULID
                // embedded in its content (`attributes.blockId`). This proves every
                // section opens schema-valid (>=1 block) and that block identity is
                // stable, not minted fresh on each persist.
                let block_rows = crate::entities::blocks::Entity::find()
                    .filter(
                        crate::entities::blocks::Column::PageId
                            .eq(PageIdCol::from(page_id.clone())),
                    )
                    .all(&conn)
                    .await
                    .expect("query blocks");
                assert_eq!(
                    block_rows.len(),
                    PageKind::Entity.sections().len(),
                    "home page seeded with one block per section",
                );
                for block in &block_rows {
                    let row_id = BlockId::from(block.id.clone()).to_string();
                    let content: serde_json::Value =
                        serde_json::from_slice(&block.content).expect("seed block content is JSON");
                    assert_eq!(
                        content["attributes"]["blockId"].as_str(),
                        Some(row_id.as_str()),
                        "block row id equals the blockId embedded in its content",
                    );
                    assert_eq!(content["nodeName"].as_str(), Some("paragraph"));
                }
                use familiar_systems_campaign_shared::loro::page::Section;
                let mut seeded_sections: Vec<&str> = block_rows
                    .iter()
                    .map(|b| Section::from(b.section).as_str())
                    .collect();
                seeded_sections.sort();
                let mut expected_sections: Vec<&str> = PageKind::Entity
                    .sections()
                    .iter()
                    .map(|s| s.as_str())
                    .collect();
                expected_sections.sort();
                assert_eq!(
                    seeded_sections, expected_sections,
                    "one block seeded in each declared section",
                );

                return page_id;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("home base was not seeded within timeout");
    }

    #[tokio::test]
    async fn brand_new_campaign_seeds_home_base() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let campaign_id = CampaignId::generate();
        let args = fast_args(campaign_id.clone(), store, 60_000, 60_000);
        let actor_ref = CampaignSupervisor::spawn(args);
        actor_ref.wait_for_startup().await;

        let db_path = tmp.path().join(format!("{}.db", campaign_id.0));
        poll_until_seeded(&db_path).await;

        actor_ref.stop_gracefully().await.unwrap();
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn reopen_does_not_reseed_home_base() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let campaign_id = CampaignId::generate();
        let db_path = tmp.path().join(format!("{}.db", campaign_id.0));

        // First open seeds the base camp.
        let first = CampaignSupervisor::spawn(fast_args(
            campaign_id.clone(),
            store.clone(),
            60_000,
            60_000,
        ));
        first.wait_for_startup().await;
        let seeded = poll_until_seeded(&db_path).await;
        first.stop_gracefully().await.unwrap();
        first.wait_for_shutdown_with_result(|_| ()).await;

        // Reopen is a cold checkout (`is_new == false`): the existing metadata
        // row means the seed guard does not fire, so no second base camp.
        let second = CampaignSupervisor::spawn(fast_args(
            campaign_id.clone(),
            store.clone(),
            60_000,
            60_000,
        ));
        second.wait_for_startup().await;
        // Give any (erroneous) seed task a chance to run before asserting.
        tokio::time::sleep(Duration::from_millis(200)).await;

        let conn = crate::db::connect_readonly(&db_path)
            .await
            .expect("open readonly");
        let pages = pages::Entity::find().all(&conn).await.expect("query pages");
        assert_eq!(pages.len(), 1, "reopen must not add a second base camp");
        let meta = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&conn)
            .await
            .expect("query metadata")
            .expect("metadata row exists");
        assert_eq!(
            meta.home_page_id.map(PageId::from),
            Some(seeded),
            "home pointer unchanged on reopen"
        );

        second.stop_gracefully().await.unwrap();
        second.wait_for_shutdown_with_result(|_| ()).await;
    }

    /// Pages must have a non-empty title. An empty or whitespace-only name is
    /// rejected before anything is persisted, on every creation path (not just
    /// the UI).
    #[tokio::test]
    async fn create_page_rejects_empty_name() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
        let supervisor = CampaignSupervisor::spawn(args);
        supervisor.wait_for_startup().await;

        for name in ["", "   "] {
            let err = supervisor
                .ask(CreatePage {
                    name: name.to_string(),
                    status: Some(Status::GmOnly),
                    parent: None,
                    kind: DocumentPageKind::Entity,
                })
                .await
                .expect_err("empty name must be rejected");
            assert!(
                matches!(err, SendError::HandlerError(CreatePageError::EmptyName)),
                "expected EmptyName, got {err:?}"
            );
        }

        supervisor.stop_gracefully().await.unwrap();
        supervisor.wait_for_shutdown_with_result(|_| ()).await;
    }

    /// Sessions are named like every other page kind: an empty or whitespace-only
    /// name is rejected before anything is persisted (the empty-name guard is no
    /// longer entity/template-only).
    #[tokio::test]
    async fn create_session_rejects_empty_name() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let store = store_in(tmp.path());
        let args = fast_args(CampaignId::generate(), store, 60_000, 60_000);
        let supervisor = CampaignSupervisor::spawn(args);
        supervisor.wait_for_startup().await;

        for name in ["", "   "] {
            let err = supervisor
                .ask(CreateSession {
                    name: name.to_string(),
                    status: Some(Status::GmOnly),
                    parent: None,
                })
                .await
                .expect_err("empty session name must be rejected");
            assert!(
                matches!(err, SendError::HandlerError(CreateSessionError::EmptyName)),
                "expected EmptyName, got {err:?}"
            );
        }

        supervisor.stop_gracefully().await.unwrap();
        supervisor.wait_for_shutdown_with_result(|_| ()).await;
    }
}
