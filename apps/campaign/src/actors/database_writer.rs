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

use familiar_systems_campaign_shared::loro::thing::SECTION_CONTENT;

use crate::domain::thing::NewThing;
use crate::entities::columns::{BlockIdCol, StatusCol};
use crate::entities::{blocks, campaign_metadata, things, toc_entries};

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

/// Point `campaign_metadata.home_thing_id` at a Thing. Partial update: touches
/// only `home_thing_id` and `updated_at`, leaving every other field as-is. No
/// existence check: the sole caller (the genesis seed) passes a just-committed
/// Thing, and the FK (`ON DELETE SET NULL`) keeps the pointer honest over the
/// Thing's lifetime. Reuses [`PatchCampaignError`]; the only failure modes are
/// a missing metadata row or a DB error.
#[derive(Debug, Clone)]
pub struct DbSetLandingPage {
    pub thing_id: ThingId,
}

impl Message<DbSetLandingPage> for DatabaseWriteActor {
    type Reply = Result<(), PatchCampaignError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %msg.thing_id.0),
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
        am.home_thing_id = Set(Some(ThingIdCol::from(msg.thing_id)));
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
                            toc_entries::Column::ThingId,
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
// WriteThingBlocks
// ---------------------------------------------------------------------------

use familiar_systems_campaign_shared::id::ThingId;

use crate::entities::columns::ThingIdCol;

pub struct WriteThingBlocks {
    pub thing_id: ThingId,
    pub blocks: Vec<blocks::ActiveModel>,
    pub name_sync: Option<String>,
}

impl Message<WriteThingBlocks> for DatabaseWriteActor {
    type Reply = Result<(), sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %msg.thing_id.0),
    )]
    async fn handle(
        &mut self,
        msg: WriteThingBlocks,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let block_count = msg.blocks.len();
        tracing::debug!(block_count, "writing thing blocks");

        // Delete-then-insert is a full-snapshot replace, so it must be atomic:
        // the delete truncates every block for this Thing, and a failed insert
        // would otherwise leave the table empty with no in-memory copy to
        // recover from (this flush also runs on actor stop, as the Loro doc is
        // torn down). One transaction; an error before commit rolls back the
        // delete. Mirrors `DbCreateThing`.
        let txn = self.conn.begin().await?;

        blocks::Entity::delete_many()
            .filter(blocks::Column::ThingId.eq(ThingIdCol::from(msg.thing_id.clone())))
            .exec(&txn)
            .await?;

        if !msg.blocks.is_empty() {
            blocks::Entity::insert_many(msg.blocks).exec(&txn).await?;
        }

        if let Some(name) = msg.name_sync {
            things::Entity::update_many()
                .filter(things::Column::Id.eq(ThingIdCol::from(msg.thing_id.clone())))
                .col_expr(things::Column::Name, sea_orm::sea_query::Expr::value(name))
                .col_expr(
                    things::Column::UpdatedAt,
                    sea_orm::sea_query::Expr::value(Utc::now()),
                )
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;
        tracing::debug!(block_count, "thing blocks written");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// DbCreateThing (genesis write)
// ---------------------------------------------------------------------------

/// Persist a brand-new Thing: its `things` row plus any seeded `blocks`, in a
/// single transaction. Invoked once, from the `ThingActor`'s genesis path
/// (`ThingInit::New`), so the actor that owns the Thing owns its birth write.
/// Replies with the persisted row (timestamps stamped here, at the write edge).
pub struct DbCreateThing {
    pub new_thing: NewThing,
}

impl Message<DbCreateThing> for DatabaseWriteActor {
    type Reply = Result<things::Model, sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, thing_id = %msg.new_thing.id.0),
    )]
    async fn handle(
        &mut self,
        msg: DbCreateThing,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let nt = msg.new_thing;
        let thing_id_col = ThingIdCol::from(nt.id.clone());
        let block_count = nt.blocks.len();
        let now = Utc::now();

        tracing::debug!(block_count, "creating thing");

        let txn = self.conn.begin().await?;

        let model = things::ActiveModel {
            id: Set(thing_id_col.clone()),
            name: Set(nt.name),
            status: Set(StatusCol::from(nt.status)),
            prototype_id: Set(nt.prototype_id.map(ThingIdCol::from)),
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
                    thing_id: Set(thing_id_col.clone()),
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
        tracing::debug!(block_count, "thing created");
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
            home_thing_id: Set(None),
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
    async fn create_thing_inserts_row_and_blocks_atomically() {
        use crate::domain::thing::{NewBlock, NewThing};
        use familiar_systems_campaign_shared::id::{BlockId, ThingId};
        use familiar_systems_campaign_shared::status::Status;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: CampaignId::generate(),
            conn: conn.clone(),
        });

        let thing_id = ThingId::generate();
        let model = actor
            .ask(DbCreateThing {
                new_thing: NewThing {
                    id: thing_id.clone(),
                    name: "Korgath".into(),
                    status: Status::GmOnly,
                    prototype_id: None,
                    blocks: vec![NewBlock {
                        id: BlockId::generate(),
                        ordering: 0,
                        content: b"hello".to_vec(),
                        status: Status::GmOnly,
                    }],
                },
            })
            .await
            .expect("create thing");
        assert_eq!(model.name, "Korgath");

        let things = things::Entity::find().all(&conn).await.unwrap();
        assert_eq!(things.len(), 1, "thing row inserted");
        let block_rows = blocks::Entity::find().all(&conn).await.unwrap();
        assert_eq!(block_rows.len(), 1, "block row inserted");
        assert_eq!(block_rows[0].content, b"hello");
    }

    /// A `WriteThingBlocks` whose insert fails must not destroy the blocks it
    /// was meant to replace. The handler deletes the Thing's existing blocks
    /// before inserting the new set; without a transaction a failed insert
    /// leaves the table truncated (the catastrophic case, since this runs on
    /// flush-on-stop while the in-memory Loro doc is being torn down).
    #[tokio::test]
    async fn write_thing_blocks_failed_insert_preserves_existing_rows() {
        use familiar_systems_campaign_shared::id::BlockId;

        db::register_sqlite_vec();
        let conn = db::connect("sqlite::memory:").await.expect("sqlite");
        Migrator::up(&conn, None).await.expect("migrate");
        let actor = DatabaseWriteActor::spawn(DatabaseWriteActorArgs {
            campaign_id: CampaignId::generate(),
            conn: conn.clone(),
        });

        let now = Utc::now();
        let thing_id = ThingId::generate();
        let thing_id_col = ThingIdCol::from(thing_id.clone());

        // FK parent: blocks.thing_id -> things.id.
        things::ActiveModel {
            id: Set(thing_id_col.clone()),
            name: Set("Korgath".into()),
            status: Set(StatusCol::GmOnly),
            prototype_id: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        }
        .insert(&conn)
        .await
        .expect("seed thing");

        // One pre-existing block a correct (transactional) write must preserve.
        blocks::ActiveModel {
            id: Set(BlockIdCol::from(BlockId::generate())),
            thing_id: Set(thing_id_col.clone()),
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

        // A doomed snapshot: two rows share one primary key, so `insert_many`
        // fails as a unit (PK uniqueness, no FK dependency). The handler has
        // already deleted the existing block by then.
        let dup_id = BlockId::generate();
        let doomed = vec![
            blocks::ActiveModel {
                id: Set(BlockIdCol::from(dup_id.clone())),
                thing_id: Set(thing_id_col.clone()),
                status: Set(StatusCol::GmOnly),
                ordering: Set(0),
                content: Set(b"new-a".to_vec()),
                section: Set(SECTION_CONTENT.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            },
            blocks::ActiveModel {
                id: Set(BlockIdCol::from(dup_id.clone())),
                thing_id: Set(thing_id_col.clone()),
                status: Set(StatusCol::GmOnly),
                ordering: Set(1),
                content: Set(b"new-b".to_vec()),
                section: Set(SECTION_CONTENT.to_string()),
                created_at: Set(now),
                updated_at: Set(now),
            },
        ];

        actor
            .ask(WriteThingBlocks {
                thing_id: thing_id.clone(),
                blocks: doomed,
                name_sync: None,
            })
            .await
            .expect_err("duplicate-PK insert must fail");

        // The original block survives: the failed write rolled back its delete.
        let rows = blocks::Entity::find().all(&conn).await.unwrap();
        assert_eq!(rows.len(), 1, "existing block must survive a failed write");
        assert_eq!(rows[0].content, b"original");
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
            thing_id: Set(None),
            folder_title: Set(Some("Stale Folder".into())),
            visibility: Set(StatusCol::GmOnly),
            parent_id: Set(None),
            position: Set(0),
        }
        .insert(&conn)
        .await
        .expect("seed stale toc row");

        // Keep-set is just "new"; its thing_id points at a Thing that doesn't
        // exist, so the upsert trips the FK (foreign_keys are ON) and fails.
        // The prune deletes "old" first.
        let ghost = ThingId::generate();
        let rows = vec![toc_entries::ActiveModel {
            id: Set("new".to_string()),
            thing_id: Set(Some(ThingIdCol::from(ghost))),
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
