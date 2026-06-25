//! `DatabaseWriteActor`: single owner of the per-campaign sea-orm write
//! connection.
//!
//! Every write to the campaign DB flows through this actor's mailbox.

use chrono::Utc;
use familiar_systems_app_shared::id::CampaignId;
use kameo::prelude::{Actor, Context, Message};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, TransactionTrait,
};

use crate::domain::page::NewPage;
use crate::domain::session::next_session_ordinal;
use crate::entities::columns::{BlockIdCol, PageKindCol, SectionCol, SessionIdCol, StatusCol};
use crate::entities::{blocks, campaign_metadata, pages, sessions, toc_entries};

use super::database_writer_relationships::{
    RelationshipWrite, RelationshipWriteError, RelationshipWriteOutcome, delete_relationship,
    insert_relationship, set_knowledge, set_session_stamp,
};

pub struct DatabaseWriteActor {
    campaign_id: CampaignId,
    conn: DatabaseConnection,
}

pub struct DatabaseWriteActorArgs {
    pub campaign_id: CampaignId,
    pub conn: DatabaseConnection,
}

impl Actor for DatabaseWriteActor {
    type Args = DatabaseWriteActorArgs;
    type Error = std::convert::Infallible;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %args.campaign_id.0),
    )]
    async fn on_start(
        args: Self::Args,
        _actor_ref: kameo::actor::ActorRef<Self>,
    ) -> Result<Self, Self::Error> {
        tracing::debug!("database actor started");
        Ok(Self {
            campaign_id: args.campaign_id,
            conn: args.conn,
        })
    }

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn on_stop(
        &mut self,
        _actor_ref: kameo::actor::WeakActorRef<Self>,
        _reason: kameo::error::ActorStopReason,
    ) -> Result<(), Self::Error> {
        tracing::debug!("database actor stopped");
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

impl Message<PatchCampaignMetadata> for DatabaseWriteActor {
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
// DbSetLandingPage
// ---------------------------------------------------------------------------

/// Point `campaign_metadata.home_page_id` at a Page. Partial update: touches
/// only `home_page_id` and `updated_at`, leaving every other field as-is. No
/// existence check: the sole caller (the genesis seed) passes a just-committed
/// Page, and the FK (`ON DELETE SET NULL`) keeps the pointer honest over the
/// Page's lifetime. Reuses [`PatchCampaignError`]; the only failure modes are
/// a missing metadata row or a DB error.
#[derive(Debug, Clone)]
pub struct DbSetLandingPage {
    pub page_id: PageId,
}

impl Message<DbSetLandingPage> for DatabaseWriteActor {
    type Reply = Result<(), PatchCampaignError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.page_id.0),
    )]
    async fn handle(
        &mut self,
        msg: DbSetLandingPage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let existing = campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&self.conn)
            .await?
            .ok_or(PatchCampaignError::NoMetadataRow)?;

        let mut am: campaign_metadata::ActiveModel = existing.into();
        am.home_page_id = Set(Some(PageIdCol::from(msg.page_id)));
        am.updated_at = Set(Utc::now());
        am.update(&self.conn).await?;
        Ok(())
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

impl Message<GetMetadata> for DatabaseWriteActor {
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
        campaign_metadata::Entity::find_by_id(campaign_metadata::METADATA_ROW_ID)
            .one(&self.conn)
            .await?
            .ok_or(MetadataError::NoMetadataRow)
    }
}

// ---------------------------------------------------------------------------
// WriteTocSnapshot
// ---------------------------------------------------------------------------

pub struct WriteTocSnapshot {
    pub rows: Vec<toc_entries::ActiveModel>,
}

impl Message<WriteTocSnapshot> for DatabaseWriteActor {
    type Reply = Result<(), sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: WriteTocSnapshot,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let row_count = msg.rows.len();
        tracing::debug!(row_count, "writing toc snapshot");

        let keep_ids: Vec<sea_orm::Value> = msg
            .rows
            .iter()
            .map(|r| r.id.clone().unwrap().into())
            .collect();

        // Prune-then-upsert is a full-snapshot replace; it must be atomic so a
        // failed upsert can't leave stale rows pruned but unreplaced. One
        // transaction wraps both; an early return drops `txn`, rolling back the
        // prune. The per-statement `tracing::error!` logging is retained.
        let txn = self.conn.begin().await?;

        if keep_ids.is_empty() {
            if let Err(e) = toc_entries::Entity::delete_many().exec(&txn).await {
                tracing::error!(error = %e, "failed to delete toc entries");
                return Err(e);
            }
        } else {
            if let Err(e) = toc_entries::Entity::delete_many()
                .filter(toc_entries::Column::Id.is_not_in(keep_ids))
                .exec(&txn)
                .await
            {
                tracing::error!(error = %e, "failed to prune stale toc entries");
                return Err(e);
            }

            if let Err(e) = toc_entries::Entity::insert_many(msg.rows)
                .on_conflict(
                    sea_orm::sea_query::OnConflict::column(toc_entries::Column::Id)
                        .update_columns([
                            toc_entries::Column::PageId,
                            toc_entries::Column::FolderTitle,
                            toc_entries::Column::Visibility,
                            toc_entries::Column::ParentId,
                            toc_entries::Column::Position,
                        ])
                        .to_owned(),
                )
                .exec(&txn)
                .await
            {
                tracing::error!(row_count, error = %e, "failed to upsert toc entries");
                return Err(e);
            }
        }

        txn.commit().await?;
        tracing::debug!(row_count, "toc snapshot written");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// WritePageBlocks
// ---------------------------------------------------------------------------

use familiar_systems_campaign_shared::id::{PageId, SessionId};

use crate::entities::columns::PageIdCol;

pub struct WritePageBlocks {
    pub page_id: PageId,
    pub blocks: Vec<blocks::ActiveModel>,
    pub name_sync: Option<String>,
}

impl Message<WritePageBlocks> for DatabaseWriteActor {
    type Reply = Result<(), sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.page_id.0),
    )]
    async fn handle(
        &mut self,
        msg: WritePageBlocks,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let block_count = msg.blocks.len();
        tracing::debug!(block_count, "writing page blocks");

        let page_id_col = PageIdCol::from(msg.page_id.clone());

        // The live block ids in this snapshot; everything else for the Page is
        // stale and gets pruned. `flush` always `Set`s the id, so the unwrap is
        // total (same idiom as `WriteTocSnapshot`).
        let keep_ids: Vec<sea_orm::Value> = msg
            .blocks
            .iter()
            .map(|b| b.id.clone().unwrap().into())
            .collect();

        // Prune-then-upsert is a full-snapshot replace; it must be atomic so a
        // failed upsert can't leave the Page's blocks pruned but unreplaced (this
        // flush also runs on actor stop, as the Loro doc is torn down). One
        // transaction wraps both; an early return drops `txn`, rolling back the
        // prune. Upsert rather than delete-then-insert so a block's `created_at`
        // survives edits: the row is updated in place and `CreatedAt` is left out
        // of the conflict-update set below. Mirrors `WriteTocSnapshot`.
        let txn = self.conn.begin().await?;

        if msg.blocks.is_empty() {
            // No live blocks: drop every block for this Page.
            blocks::Entity::delete_many()
                .filter(blocks::Column::PageId.eq(page_id_col.clone()))
                .exec(&txn)
                .await?;
        } else {
            // Prune blocks absent from the new snapshot. Scoped to this Page:
            // unlike `toc_entries` (a per-campaign singleton table), `blocks` is
            // shared across all Pages, so an unscoped `NOT IN` would delete other
            // Pages' rows. `is_not_in` binds one parameter per live block; a Page
            // with enough blocks to exceed `SQLITE_MAX_VARIABLE_NUMBER` would fail
            // here -- the same bound `WriteTocSnapshot` already accepts (page block
            // counts are bounded in practice).
            blocks::Entity::delete_many()
                .filter(blocks::Column::PageId.eq(page_id_col.clone()))
                .filter(blocks::Column::Id.is_not_in(keep_ids))
                .exec(&txn)
                .await?;

            // Upsert the snapshot. `CreatedAt` is deliberately omitted from the
            // update set so an existing block keeps its original creation time; the
            // `created_at = now` that `flush` stamps only takes effect on the insert
            // (new-block) path. `PageId` is omitted because a block never changes
            // pages. `Section` is included defensively: a block normally stays in
            // one section, but if one ever moves between a page's section editors
            // the in-place update keeps `blocks.section` truthful instead of stale.
            blocks::Entity::insert_many(msg.blocks)
                .on_conflict(
                    sea_orm::sea_query::OnConflict::column(blocks::Column::Id)
                        .update_columns([
                            blocks::Column::Status,
                            blocks::Column::Section,
                            blocks::Column::Ordering,
                            blocks::Column::Content,
                            blocks::Column::UpdatedAt,
                        ])
                        .to_owned(),
                )
                .exec(&txn)
                .await?;
        }

        if let Some(name) = msg.name_sync {
            pages::Entity::update_many()
                .filter(pages::Column::Id.eq(page_id_col))
                .col_expr(pages::Column::Name, sea_orm::sea_query::Expr::value(name))
                .col_expr(
                    pages::Column::UpdatedAt,
                    sea_orm::sea_query::Expr::value(Utc::now()),
                )
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;
        tracing::debug!(block_count, "page blocks written");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DbCreatePage (genesis write)
// ---------------------------------------------------------------------------

/// Persist a brand-new Page: its `pages` row plus any seeded `blocks`, in a
/// single transaction. Invoked once, from the `PageActor`'s genesis path
/// (`PageInit::New`), so the actor that owns the Page owns its birth write.
/// Replies with the persisted row (timestamps stamped here, at the write edge).
pub struct DbCreatePage {
    pub new_page: NewPage,
}

impl Message<DbCreatePage> for DatabaseWriteActor {
    type Reply = Result<pages::Model, sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.new_page.id.0),
    )]
    async fn handle(
        &mut self,
        msg: DbCreatePage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let nt = msg.new_page;
        let page_id_col = PageIdCol::from(nt.id.clone());
        let block_count = nt.blocks.len();
        let now = Utc::now();

        tracing::debug!(block_count, "creating page");

        let txn = self.conn.begin().await?;

        let model = pages::ActiveModel {
            id: Set(page_id_col.clone()),
            name: Set(nt.name),
            status: Set(StatusCol::from(nt.status)),
            kind: Set(PageKindCol::from(nt.kind)),
            template_id: Set(nt.template_id.map(PageIdCol::from)),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;

        if !nt.blocks.is_empty() {
            let block_rows: Vec<blocks::ActiveModel> = nt
                .blocks
                .into_iter()
                .map(|b| blocks::ActiveModel {
                    id: Set(BlockIdCol::from(b.id)),
                    page_id: Set(page_id_col.clone()),
                    status: Set(StatusCol::from(b.status)),
                    ordering: Set(b.ordering),
                    content: Set(b.content),
                    section: Set(SectionCol::from(b.section)),
                    created_at: Set(now),
                    updated_at: Set(now),
                })
                .collect();
            blocks::Entity::insert_many(block_rows).exec(&txn).await?;
        }

        txn.commit().await?;
        tracing::debug!(block_count, "page created");
        Ok(model)
    }
}

// ---------------------------------------------------------------------------
// DbCreateSession (session genesis write)
// ---------------------------------------------------------------------------

/// Persist a brand-new session atomically: its `pages` row, the seeded section
/// `blocks`, and the temporal `sessions` row, in one transaction. Invoked once,
/// from the `PageActor`'s session-genesis path (`PageInit::NewSession`), so the
/// actor that owns the Session page owns its birth write.
///
/// This is the effectful half of "mint a session". Page-first insert order
/// satisfies the `sessions.page_id -> pages.id` FK; the ordinal is assigned
/// `max + 1` via the pure [`next_session_ordinal`] kernel; and the `SessionId`
/// is generated here, inside the txn, because nothing upstream consumes it.
pub struct DbCreateSession {
    /// The Session page to persist (`kind == Session`, blocks already seeded).
    /// Its `name` is the session's label (required and non-blank); the `sessions`
    /// row stays purely temporal and stores no name.
    pub new_page: NewPage,
}

/// What a session genesis produced: the persisted `pages` row and its temporal
/// `sessions` row. The `PageActor`'s genesis path threads this back to the
/// supervisor through a reply oneshot, so the HTTP response is built without a
/// read-after-write round-trip on the reader pool.
#[derive(Debug, Clone, kameo::Reply)]
pub struct CreatedSession {
    pub page: pages::Model,
    pub session: sessions::Model,
}

impl Message<DbCreateSession> for DatabaseWriteActor {
    type Reply = Result<CreatedSession, sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.new_page.id.0),
    )]
    async fn handle(
        &mut self,
        msg: DbCreateSession,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let nt = msg.new_page;
        let page_id_col = PageIdCol::from(nt.id.clone());
        let block_count = nt.blocks.len();
        let now = Utc::now();

        tracing::debug!(block_count, "creating session");

        let txn = self.conn.begin().await?;

        // 1. The page first: the `sessions.page_id` FK requires it to exist.
        let page = pages::ActiveModel {
            id: Set(page_id_col.clone()),
            name: Set(nt.name),
            status: Set(StatusCol::from(nt.status)),
            kind: Set(PageKindCol::from(nt.kind)),
            template_id: Set(nt.template_id.map(PageIdCol::from)),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&txn)
        .await?;

        // 2. The seeded section blocks.
        if !nt.blocks.is_empty() {
            let block_rows: Vec<blocks::ActiveModel> = nt
                .blocks
                .into_iter()
                .map(|b| blocks::ActiveModel {
                    id: Set(BlockIdCol::from(b.id)),
                    page_id: Set(page_id_col.clone()),
                    status: Set(StatusCol::from(b.status)),
                    ordering: Set(b.ordering),
                    content: Set(b.content),
                    section: Set(SectionCol::from(b.section)),
                    created_at: Set(now),
                    updated_at: Set(now),
                })
                .collect();
            blocks::Entity::insert_many(block_rows).exec(&txn).await?;
        }

        // 3. The temporal row. Read the campaign's current highest ordinal
        //    in-txn, assign `max + 1` via the pure kernel, mint the id here.
        let prev_max = sessions::Entity::find()
            .order_by_desc(sessions::Column::Ordinal)
            .one(&txn)
            .await?
            .map(|m| m.ordinal);
        let ordinal =
            next_session_ordinal(prev_max).map_err(|e| sea_orm::DbErr::Custom(e.to_string()))?;
        let session = sessions::ActiveModel {
            id: Set(SessionIdCol::from(SessionId::generate())),
            ordinal: Set(ordinal),
            created_at: Set(now),
            // Equal at genesis; the future reorder op is what diverges them.
            updated_at: Set(now),
            page_id: Set(Some(page_id_col)),
        }
        .insert(&txn)
        .await?;

        txn.commit().await?;
        tracing::debug!(block_count, ordinal, "session created");
        Ok(CreatedSession { page, session })
    }
}

/// Apply an ordered list of relationship writes in one transaction, all-or-nothing.
/// The actor decomposes each GM op into this list (supersede = `[Create, SetStamp]`,
/// a multi-axis patch a list of `SetStamp`s, the single ops a one-element list); the
/// writer commits them together or rolls the whole batch back. Outcomes are returned
/// in write order.
///
/// This is the relationship analogue of [`DbCreateSession`]: a `begin`/`commit`
/// multi-statement write that returns the committed rows. A duplicate *live* fact
/// trips the partial unique index and rolls the batch back, mapped to
/// [`RelationshipWriteError::DuplicateLiveFact`].
pub struct ApplyRelationshipWrites {
    pub writes: Vec<RelationshipWrite>,
}

impl Message<ApplyRelationshipWrites> for DatabaseWriteActor {
    type Reply = Result<Vec<RelationshipWriteOutcome>, RelationshipWriteError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, writes = msg.writes.len()),
    )]
    async fn handle(
        &mut self,
        msg: ApplyRelationshipWrites,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        // One `now` for the whole batch, so the rows a single op writes share a
        // timestamp. Any error drops `txn` (rollback) via `?` before `commit`.
        let now = Utc::now();
        let txn = self.conn.begin().await?;
        let mut outcomes = Vec::with_capacity(msg.writes.len());
        for write in msg.writes {
            let outcome = match write {
                RelationshipWrite::Create(new) => RelationshipWriteOutcome::Upserted(Box::new(
                    insert_relationship(&txn, new, now).await?,
                )),
                RelationshipWrite::SetStamp { rel_id, column, at } => {
                    RelationshipWriteOutcome::Upserted(Box::new(
                        set_session_stamp(&txn, rel_id, column, at).await?,
                    ))
                }
                RelationshipWrite::SetKnowledge { rel_id, knowledge } => {
                    RelationshipWriteOutcome::Upserted(Box::new(
                        set_knowledge(&txn, rel_id, knowledge).await?,
                    ))
                }
                RelationshipWrite::Delete { rel_id } => {
                    delete_relationship(&txn, rel_id.clone()).await?;
                    RelationshipWriteOutcome::Removed(rel_id)
                }
            };
            outcomes.push(outcome);
        }
        txn.commit().await?;
        Ok(outcomes)
    }
}

// ---------------------------------------------------------------------------
// Ping (health check / test)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ping;

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub struct Pong;

impl Message<Ping> for DatabaseWriteActor {
    type Reply = Pong;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        Pong
    }
}
