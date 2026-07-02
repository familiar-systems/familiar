//! Page and session creation: the supervisor's `CreatePage` / `CreateSession`
//! orchestration (validate placement, spawn the owning `PageActor` in genesis
//! mode, register + link it, place its ToC node).

use std::time::{Duration, Instant};

use familiar_systems_campaign_shared::id::PageId;
use familiar_systems_campaign_shared::loro::toc::TocPageKind;
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use kameo::actor::Spawn;
use kameo::message::{Context, Message};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tokio::sync::oneshot;

use super::CampaignSupervisor;
use crate::actors::database_writer::CreatedSession;
use crate::actors::page::{PageActor, PageActorArgs, PageInit};
use crate::actors::toc::{AddPageNode, ResolvePageNode, SeedTocChild, SeedTocFolder};
use crate::domain::page::DocumentPageKind;
use crate::entities::columns::PageKindCol;
use crate::entities::pages;
use crate::starter_content::compile::CompiledTemplate;

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
            // A GM-created page starts blank; template seeding uses its own
            // genesis path with compiled `seed` blocks.
            seed: Vec::new(),
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
// SeedTemplateBundle
// ---------------------------------------------------------------------------

/// Seed a system's template bundle at wizard completion: create each compiled
/// template as a `template`-kind Page and nest them under a fresh ToC folder.
///
/// The route compiles the bundle (it alone reaches the catalog) and fires this
/// best-effort; the client is not made to wait, since the seeded folder and pages
/// reach it through the live ToC broadcast, and a failure self-heals like any
/// create (an orphaned page re-surfaces at the ToC root on the next checkout).
/// This reuses the `CreatePage` genesis-spawn shape but replaces the per-page
/// `AddPageNode` with one `SeedTocFolder` at the end, so the single-writer
/// invariant still holds: each Page persists its own birth row through its owning
/// actor; nothing writes a Page's rows around it.
#[derive(Debug)]
pub struct SeedTemplateBundle {
    pub folder_title: String,
    pub templates: Vec<CompiledTemplate>,
}

impl Message<SeedTemplateBundle> for CampaignSupervisor {
    // Best-effort: failures are logged, there is nothing for the caller to act on.
    type Reply = ();

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, template_count = msg.templates.len()),
    )]
    async fn handle(
        &mut self,
        msg: SeedTemplateBundle,
        ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.last_activity = Instant::now();
        if msg.templates.is_empty() {
            return;
        }

        let (db_reader, db_writer) = {
            let db = self
                .db
                .as_ref()
                .expect("db must be Some while actor is running");
            (db.reader().clone(), db.writer().clone())
        };

        let mut children = Vec::with_capacity(msg.templates.len());
        for template in msg.templates {
            let page_id = PageId::generate();
            // The genesis reply (the committed `pages::Model`) is unused here -- we
            // already hold everything the ToC node needs -- so drop the receiver;
            // genesis tolerates a gone receiver (`let _ = reply.send(..)`).
            let (reply_tx, _) = oneshot::channel();
            let actor = PageActor::spawn(PageActorArgs {
                campaign_id: self.campaign_id.clone(),
                page_id: page_id.clone(),
                db_reader: db_reader.clone(),
                db_writer: db_writer.clone(),
                toc: self.toc.clone(),
                init: PageInit::NewDocumentPage {
                    name: template.name.clone(),
                    kind: DocumentPageKind::Template,
                    status: Status::GmOnly,
                    seed: template.blocks,
                    reply: reply_tx,
                },
                debounce_duration: Duration::from_secs(2),
                idle_timeout: Duration::from_secs(30),
            });
            actor.wait_for_startup().await;
            if !actor.is_alive() {
                tracing::error!(name = %template.name, "template page genesis failed; skipping");
                continue;
            }
            self.pages.insert(page_id.clone(), actor.clone());
            // Link after insert so `on_link_died` prunes on idle self-eviction.
            ctx.actor_ref().clone().link(&actor).await;

            children.push(SeedTocChild {
                page_id,
                title: template.name,
                page_kind: TocPageKind::Template,
                visibility: Status::GmOnly,
            });
        }

        if children.is_empty() {
            return;
        }

        // One folder holding the whole bundle; the pages are already persisted, so
        // a ToC failure just leaves them to re-surface at the root on checkout.
        if let Err(e) = self
            .toc
            .ask(SeedTocFolder {
                folder_title: msg.folder_title,
                folder_visibility: Status::GmOnly,
                children,
            })
            .await
        {
            tracing::error!(
                error = %e,
                "failed to place seeded templates in a toc folder; they self-heal at root on next checkout"
            );
        }
    }
}
