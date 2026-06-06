//! `DatabaseWriteActor`: single owner of the per-campaign sea-orm write
//! connection.
//!
//! Every write to the campaign DB flows through this actor's mailbox.

use chrono::Utc;
use familiar_systems_app_shared::id::CampaignId;
use kameo::prelude::{Actor, Context, Message};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    TransactionTrait,
};

use familiar_systems_campaign_shared::loro::page::SECTION_CONTENT;

use crate::domain::page::NewPage;
use crate::entities::columns::{BlockIdCol, PageKindCol, StatusCol};
use crate::entities::{blocks, campaign_metadata, pages, toc_entries};

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

use familiar_systems_campaign_shared::id::PageId;

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
            // (new-block) path. `PageId`/`Section` are constant per block.
            blocks::Entity::insert_many(msg.blocks)
                .on_conflict(
                    sea_orm::sea_query::OnConflict::column(blocks::Column::Id)
                        .update_columns([
                            blocks::Column::Status,
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
                    section: Set(SECTION_CONTENT.to_string()),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::migrations::Migrator;
    use kameo::actor::Spawn;
    use sea_orm_migration::MigratorTrait;

    async fn spawn_with_migrations() -> (kameo::actor::ActorRef<DatabaseWriteActor>, CampaignId) {
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
            home_page_id: Set(None),
            wizard_completed_at: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .expect("seed metadata");

        let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
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

    #[tokio::test]
    async fn create_page_inserts_row_and_blocks_atomically() {
        use crate::domain::page::{NewBlock, NewPage};
        use familiar_systems_campaign_shared::id::{BlockId, PageId};
        use familiar_systems_campaign_shared::page_kind::PageKind;
        use familiar_systems_campaign_shared::status::Status;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: CampaignId::generate(),
            conn: conn.clone(),
        });

        let page_id = PageId::generate();
        let model = actor
            .ask(DbCreatePage {
                new_page: NewPage {
                    id: page_id.clone(),
                    name: "Korgath".into(),
                    status: Status::GmOnly,
                    kind: PageKind::Entity,
                    template_id: None,
                    blocks: vec![NewBlock {
                        id: BlockId::generate(),
                        ordering: 0,
                        content: b"hello".to_vec(),
                        status: Status::GmOnly,
                    }],
                },
            })
            .await
            .expect("create page");
        assert_eq!(model.name, "Korgath");

        let pages = pages::Entity::find().all(&conn).await.unwrap();
        assert_eq!(pages.len(), 1, "page row inserted");
        let block_rows = blocks::Entity::find().all(&conn).await.unwrap();
        assert_eq!(block_rows.len(), 1, "block row inserted");
        assert_eq!(block_rows[0].content, b"hello");
    }

    /// A `WritePageBlocks` whose upsert fails must not destroy the blocks it
    /// was meant to replace. The handler prunes the Page's stale blocks before
    /// upserting the new set; without a transaction a failed upsert leaves the
    /// blocks pruned but unreplaced (the catastrophic case, since this runs on
    /// flush-on-stop while the in-memory Loro doc is being torn down).
    #[tokio::test]
    async fn write_page_blocks_failed_insert_preserves_existing_rows() {
        use familiar_systems_campaign_shared::id::BlockId;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: CampaignId::generate(),
            conn: conn.clone(),
        });

        let now = Utc::now();
        let page_id = PageId::generate();
        let page_id_col = PageIdCol::from(page_id.clone());

        // FK parent: blocks.page_id -> pages.id.
        pages::ActiveModel {
            id: Set(page_id_col.clone()),
            name: Set("Korgath".into()),
            status: Set(StatusCol::GmOnly),
            kind: Set(PageKindCol::Entity),
            template_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .expect("seed page");

        // One pre-existing block a correct (transactional) write must preserve.
        blocks::ActiveModel {
            id: Set(BlockIdCol::from(BlockId::generate())),
            page_id: Set(page_id_col.clone()),
            status: Set(StatusCol::GmOnly),
            ordering: Set(0),
            content: Set(b"original".to_vec()),
            section: Set(SECTION_CONTENT.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .expect("seed existing block");

        // A doomed snapshot: one row whose `page_id` points at a Page that
        // doesn't exist, so the upsert trips the FK (foreign_keys are ON) and
        // fails as a unit. The handler has already pruned the existing block by
        // then. (A duplicate-PK batch no longer suffices to force failure:
        // `ON CONFLICT DO UPDATE` would resolve the conflict instead of erroring.)
        let ghost = PageId::generate();
        let doomed = vec![blocks::ActiveModel {
            id: Set(BlockIdCol::from(BlockId::generate())),
            page_id: Set(PageIdCol::from(ghost)),
            status: Set(StatusCol::GmOnly),
            ordering: Set(0),
            content: Set(b"new-a".to_vec()),
            section: Set(SECTION_CONTENT.to_string()),
            created_at: Set(now),
            updated_at: Set(now),
        }];

        actor
            .ask(WritePageBlocks {
                page_id: page_id.clone(),
                blocks: doomed,
                name_sync: None,
            })
            .await
            .expect_err("FK-violating upsert must fail");

        // The original block survives: the failed write rolled back its prune.
        let rows = blocks::Entity::find().all(&conn).await.unwrap();
        assert_eq!(rows.len(), 1, "existing block must survive a failed write");
        assert_eq!(rows[0].content, b"original");
    }

    /// `created_at` survives edits, and blocks dropped from a later snapshot are
    /// pruned. The upsert path updates an existing row in place (omitting
    /// `CreatedAt` from the conflict-update set, so the original creation time is
    /// kept) while the `NOT IN` prune removes blocks absent from the new snapshot.
    /// Regression test for "created_at reset to now on every flush" (#2 in the PR
    /// review): an editor flushes a Page's blocks repeatedly, and `created_at`
    /// must not be clobbered each time.
    #[tokio::test]
    async fn write_page_blocks_preserves_created_at_and_prunes_dropped() {
        use familiar_systems_campaign_shared::id::BlockId;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: CampaignId::generate(),
            conn: conn.clone(),
        });

        let page_id = PageId::generate();
        let page_id_col = PageIdCol::from(page_id.clone());

        // Fixed whole-second timestamps so the SQLite text round-trip is exact and
        // the test is deterministic (no `Utc::now()` sub-second flake).
        let t0 = chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap();
        let t1 = chrono::DateTime::from_timestamp(1_700_000_060, 0).unwrap();

        // FK parent: blocks.page_id -> pages.id.
        pages::ActiveModel {
            id: Set(page_id_col.clone()),
            name: Set("Korgath".into()),
            status: Set(StatusCol::GmOnly),
            kind: Set(PageKindCol::Entity),
            template_id: Set(None),
            created_at: Set(t0),
            updated_at: Set(t0),
        }
        .insert(&conn)
        .await
        .expect("seed page");

        let kept = BlockId::generate();
        let dropped = BlockId::generate();
        let row =
            |id: &BlockId, ord: i64, body: &[u8], ts: chrono::DateTime<Utc>| blocks::ActiveModel {
                id: Set(BlockIdCol::from(id.clone())),
                page_id: Set(page_id_col.clone()),
                status: Set(StatusCol::GmOnly),
                ordering: Set(ord),
                content: Set(body.to_vec()),
                section: Set(SECTION_CONTENT.to_string()),
                created_at: Set(ts),
                updated_at: Set(ts),
            };

        // First flush: two blocks, both stamped t0.
        actor
            .ask(WritePageBlocks {
                page_id: page_id.clone(),
                blocks: vec![row(&kept, 0, b"v1", t0), row(&dropped, 1, b"d1", t0)],
                name_sync: None,
            })
            .await
            .expect("first flush");

        // Second flush: `kept` is edited and carries a *later* created_at (as a
        // real flush would, stamping `now`); `dropped` is gone from the snapshot.
        actor
            .ask(WritePageBlocks {
                page_id: page_id.clone(),
                blocks: vec![row(&kept, 0, b"v2", t1)],
                name_sync: None,
            })
            .await
            .expect("second flush");

        let rows = blocks::Entity::find().all(&conn).await.unwrap();
        assert_eq!(
            rows.len(),
            1,
            "block dropped from the snapshot must be pruned"
        );
        let b = &rows[0];
        assert_eq!(b.content, b"v2", "surviving block has the latest content");
        assert_eq!(
            b.created_at, t0,
            "created_at must be preserved across the edit, not reset to the flush time"
        );
        assert_eq!(
            b.updated_at, t1,
            "updated_at must advance to the latest flush"
        );
    }

    /// A `WriteTocSnapshot` whose insert fails must leave the prior rows intact.
    /// The handler prunes stale rows before upserting; without a transaction a
    /// failed upsert leaves a partially-applied snapshot (pruned but not
    /// rewritten).
    #[tokio::test]
    async fn write_toc_snapshot_failed_insert_preserves_existing_rows() {
        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: CampaignId::generate(),
            conn: conn.clone(),
        });

        // A stale folder row a correct snapshot prune would remove, but a
        // *failed* snapshot must leave untouched.
        toc_entries::ActiveModel {
            id: Set("old".to_string()),
            page_id: Set(None),
            folder_title: Set(Some("Stale Folder".into())),
            visibility: Set(StatusCol::GmOnly),
            parent_id: Set(None),
            position: Set(0),
        }
        .insert(&conn)
        .await
        .expect("seed stale toc row");

        // Keep-set is just "new"; its page_id points at a Page that doesn't
        // exist, so the upsert trips the FK (foreign_keys are ON) and fails.
        // The prune deletes "old" first.
        let ghost = PageId::generate();
        let rows = vec![toc_entries::ActiveModel {
            id: Set("new".to_string()),
            page_id: Set(Some(PageIdCol::from(ghost))),
            folder_title: Set(None),
            visibility: Set(StatusCol::GmOnly),
            parent_id: Set(None),
            position: Set(0),
        }];

        actor
            .ask(WriteTocSnapshot { rows })
            .await
            .expect_err("FK violation must fail the upsert");

        // The stale row survives: the failed write rolled back its prune.
        let surviving = toc_entries::Entity::find().all(&conn).await.unwrap();
        assert_eq!(surviving.len(), 1, "stale row must survive a failed write");
        assert_eq!(surviving[0].id, "old");
    }
}
