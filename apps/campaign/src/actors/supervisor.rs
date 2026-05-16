//! `CampaignSupervisor`: per-campaign orchestrator.
//!
//! Owns the [`DatabaseActor`] and an idle-eviction clock. Future work
//! adds child room actors (ThingActor, TocActor, AgentConversation,
//! relationship graph, vocabulary). At the time of writing there are
//! no child room actors and no operational messages besides the test
//! [`Ping`] and the self-tell [`IdleCheck`].
//!
//! Storage initialization (create data dir, open the sea-orm pool, run
//! migrations) lives in the free function [`initialize_campaign_storage`]
//! and runs in the registry's mailbox *before* the supervisor is spawned.
//! That keeps the supervisor's `on_start` infallible: by the time the
//! actor exists, its database is ready. The trade-off is that the
//! registry's mailbox serializes inits; at ~millisecond migration
//! latency on local SQLite that's a fine cost. When checkout-from-object-storage
//! lands (the `Restoring` phase from the 2026-05-04 doc), we'll move the
//! slow path back into the actor and drive it with a `oneshot`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use familiar_systems_app_shared::id::CampaignId;
use kameo::actor::{ActorRef, Spawn, WeakActorRef};
use kameo::error::ActorStopReason;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use sea_orm::DatabaseConnection;
use sea_orm_migration::MigratorTrait;

use crate::actors::database::{DatabaseActor, DatabaseActorArgs};
use crate::db;
use crate::error::InitError;
use crate::migrations::Migrator;

/// Open (or create) a campaign's on-disk database and run migrations.
///
/// Returns the path to the `.db` file and a sea-orm connection wired
/// against it. Caller is responsible for handing the connection to a
/// `DatabaseActor` so no other code holds it.
pub async fn initialize_campaign_storage(
    campaign_id: &CampaignId,
    data_dir: &Path,
) -> Result<(PathBuf, DatabaseConnection), InitError> {
    tokio::fs::create_dir_all(data_dir)
        .await
        .map_err(|source| InitError::CreateDir {
            path: data_dir.to_path_buf(),
            source,
        })?;

    let db_path = data_dir.join(format!("{}.db", campaign_id.0));
    // sqlx wants `sqlite://` + the path. `mode=rwc` lets it create the
    // file if missing; without it sqlx would return an error on first
    // open.
    let database_url = format!("sqlite://{}?mode=rwc", db_path.display());

    let started = Instant::now();
    tracing::info!(db_path = %db_path.display(), "opening campaign database");
    let conn = db::connect(&database_url)
        .await
        .map_err(|source| InitError::OpenDatabase {
            path: db_path.clone(),
            source,
        })?;

    tracing::info!("running migrations");
    Migrator::up(&conn, None)
        .await
        .map_err(InitError::Migration)?;

    tracing::info!(
        init_elapsed_ms = started.elapsed().as_millis() as u64,
        "campaign storage ready"
    );
    Ok((db_path, conn))
}

pub struct CampaignSupervisor {
    campaign_id: CampaignId,
    db_path: PathBuf,
    database: ActorRef<DatabaseActor>,
    last_activity: Instant,
    idle_timeout: Duration,
    /// Set by the IdleCheck handler before `ctx.stop()`, or by an
    /// external [`SetStopCause`] message before the registry sends
    /// `stop_gracefully`. Read by `on_stop` to choose the `cause` log
    /// tag, and used by `IdleCheck` as a "already mid-stop" guard so a
    /// late idle tick doesn't re-fire.
    stop_cause: Option<StopCause>,
}

/// Tagged reason a supervisor stopped. Distinguishes the normal-stop
/// branches in `on_stop` so log readers can tell why a campaign drained
/// without joining log streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    /// Self-eviction after exceeding `idle_timeout`.
    Idle,
    /// Registry's `BeginDrain` workflow stopped this supervisor as part
    /// of an orderly shutdown.
    Drain,
    /// Registry stopped without a `BeginDrain` first (caller misuse, or
    /// the registry itself was stopped abruptly). Cleanup ran via the
    /// fallback path in `CampaignRegistry::on_stop`.
    RegistryFallback,
}

impl StopCause {
    fn as_str(self) -> &'static str {
        match self {
            StopCause::Idle => "idle",
            StopCause::Drain => "drain",
            StopCause::RegistryFallback => "registry_fallback",
        }
    }
}

/// Tells the supervisor why it's about to stop. Send this right before
/// `stop_gracefully` so `on_stop` can log the cause.
///
/// First write wins. If the supervisor already tagged itself (an idle
/// check marked it `Idle`), a later `SetStopCause(Drain)` is ignored.
/// Whoever decided to stop first owns the reason.
#[derive(Debug, Clone, Copy)]
pub struct SetStopCause(pub StopCause);

impl Message<SetStopCause> for CampaignSupervisor {
    type Reply = ();

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
    pub db_path: PathBuf,
    pub conn: DatabaseConnection,
    pub idle_timeout: Duration,
    pub eviction_check_interval: Duration,
}

impl Actor for CampaignSupervisor {
    type Args = CampaignSupervisorArgs;
    type Error = std::convert::Infallible;

    // FIXME: move `initialize_campaign_storage` into this `on_start`
    // when the object-storage `CampaignStore` trait and the WebSocket
    // phase-subscription channel land (per
    // `docs/plans/2026-05-04-campaign-actor-domain-design.md`).
    //
    // The registry's `EnsureCampaign` handler currently awaits
    // `initialize_campaign_storage` *inside the registry's mailbox*
    // before spawning the supervisor. That keeps `on_start` infallible
    // and is acceptable while init is "create file + run migrations"
    // (sub-millisecond). It breaks in bucket-world: reloading a cold
    // campaign means downloading the `.db` from object storage, which
    // is seconds. Running that in the registry's mailbox blocks every
    // other `EnsureCampaign` (every campaign-server tenant) for the
    // duration of the download. Moving it here gives per-campaign
    // parallelism because each supervisor's `on_start` runs in its
    // own tokio task; the registry's `EnsureCampaign` shrinks to
    // spawn + link + insert + return ref, all microseconds.
    //
    // When this change happens:
    //   - `Self::Error` flips from `Infallible` to `InitError` so
    //     `on_start` can fail with a typed error.
    //   - HTTP/WebSocket handlers do a second await after
    //     `ask(EnsureCampaign)`: either `actor_ref.wait_for_startup_result()`
    //     for a one-shot "ready or failed", or subscribe to the
    //     supervisor's phase stream (push-shaped, no poll, no parallel
    //     job-status abstraction) to render "Downloading -> Restoring
    //     -> Ready" in the SPA.
    //   - `EnsureError` shrinks; `Init` variant moves to startup-result.
    //
    // Deferred at the time of writing because (a) `/internal/campaign/init`
    // is create-only and fast even after buckets land (a new campaign
    // is an empty `.db`, no download required), so there is no
    // present-day fairness benefit, and (b) the work that introduces
    // bucket downloads is the natural home for both this move and the
    // phase-stream wiring; landing them together avoids an intermediate
    // `wait_for_startup_result` API shape the WebSocket subscription
    // would replace.
    async fn on_start(args: Self::Args, actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let span = tracing::info_span!("campaign_supervisor", campaign_id = %args.campaign_id.0);
        let _guard = span.enter();

        let database = DatabaseActor::spawn(DatabaseActorArgs {
            campaign_id: args.campaign_id.clone(),
            conn: args.conn,
        });

        tracing::info!("campaign ready");

        // Periodic idle check. We tell the actor a self-message every
        // `eviction_check_interval`; the handler decides whether enough
        // idle time has elapsed and calls `ctx.stop()` to start drain.
        // The driver task exits when the mailbox closes (actor stopped).
        let timer_ref = actor_ref.clone();
        let interval = args.eviction_check_interval;
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(interval);
            // First tick fires immediately; skip it so the first real
            // check happens after one full interval.
            tick.tick().await;
            loop {
                tick.tick().await;
                if timer_ref.tell(IdleCheck).await.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            campaign_id: args.campaign_id,
            db_path: args.db_path,
            database,
            last_activity: Instant::now(),
            idle_timeout: args.idle_timeout,
            stop_cause: None,
        })
    }

    async fn on_stop(
        &mut self,
        _actor_ref: WeakActorRef<Self>,
        reason: ActorStopReason,
    ) -> Result<(), Self::Error> {
        let span = tracing::info_span!(
            "campaign_supervisor",
            campaign_id = %self.campaign_id.0,
        );
        let _guard = span.enter();
        // Explicit cause (set by IdleCheck or SetStopCause) wins; fall
        // back to deriving from the runtime stop reason for crashes /
        // kills / link-deaths where no actor code got to mark intent.
        let cause = match (self.stop_cause, &reason) {
            (Some(c), _) => c.as_str(),
            (None, ActorStopReason::Normal | ActorStopReason::SupervisorRestart) => "signal",
            (None, ActorStopReason::Killed) => "killed",
            (None, ActorStopReason::Panicked(_)) => "crash",
            (None, ActorStopReason::LinkDied { .. }) => "link_died",
        };
        tracing::info!(cause, db_path = %self.db_path.display(), "draining supervisor");
        let started = Instant::now();
        if let Err(e) = self.database.stop_gracefully().await {
            tracing::warn!(
                ?e,
                actor_kind = "database_actor",
                "actor already stopped during drain"
            );
        }
        // `_with_result` waits through the child's `on_stop`; the
        // plain `wait_for_shutdown` only waits for mailbox closure
        // and would return before the database actor's cleanup has
        // run. See the matching note in main.rs.
        self.database.wait_for_shutdown_with_result(|_| ()).await;
        tracing::info!(
            drain_elapsed_ms = started.elapsed().as_millis() as u64,
            "supervisor stopped"
        );
        Ok(())
    }
}

/// Self-tell scheduled by the idle timer. Bumps no activity; if idle
/// past threshold, transitions to draining and `ctx.stop()`s.
#[derive(Debug, Clone, Copy)]
pub struct IdleCheck;

impl Message<IdleCheck> for CampaignSupervisor {
    type Reply = ();

    async fn handle(&mut self, _: IdleCheck, ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        let span = tracing::info_span!(
            "campaign_supervisor",
            campaign_id = %self.campaign_id.0,
        );
        let _guard = span.enter();
        if self.stop_cause.is_some() {
            // Already mid-stop (either from a previous idle tick or an
            // external SetStopCause); don't re-fire.
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

/// Test/health message. Bumps `last_activity` and replies with [`Pong`].
/// Production handlers will follow the same `bump_activity` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ping;

#[derive(Debug, Clone, Copy, PartialEq, Eq, kameo::Reply)]
pub struct Pong;

impl Message<Ping> for CampaignSupervisor {
    type Reply = Pong;

    async fn handle(&mut self, _: Ping, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        self.last_activity = Instant::now();
        Pong
    }
}

/// Test-only accessor for `stop_cause`. Used to assert first-writer-wins
/// semantics on [`SetStopCause`] without depending on log scraping.
#[cfg(test)]
#[derive(Debug, Clone, Copy)]
pub struct GetStopCause;

#[cfg(test)]
impl Message<GetStopCause> for CampaignSupervisor {
    type Reply = Option<StopCause>;

    async fn handle(
        &mut self,
        _: GetStopCause,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        self.stop_cause
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::register_sqlite_vec;
    use tempfile::TempDir;

    /// Migrations include a `vec0` virtual table; the sqlite-vec extension
    /// must be auto-registered before any campaign DB is opened. Production
    /// `main.rs` does this once at startup; tests open campaign DBs from
    /// many threads, so each test that touches `initialize_campaign_storage`
    /// must do the same. `register_sqlite_vec` is idempotent (`Once`).
    fn ensure_vec0() {
        register_sqlite_vec();
    }

    fn fast_eviction_args(
        campaign_id: CampaignId,
        db_path: PathBuf,
        conn: DatabaseConnection,
        idle_ms: u64,
        check_ms: u64,
    ) -> CampaignSupervisorArgs {
        CampaignSupervisorArgs {
            campaign_id,
            db_path,
            conn,
            idle_timeout: Duration::from_millis(idle_ms),
            eviction_check_interval: Duration::from_millis(check_ms),
        }
    }

    #[tokio::test]
    async fn initialize_creates_db_file_and_runs_migrations() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        ensure_vec0();

        let tmp = TempDir::new().unwrap();
        let campaign_id = CampaignId::generate();
        let (db_path, conn) = initialize_campaign_storage(&campaign_id, tmp.path())
            .await
            .unwrap();
        assert!(db_path.exists(), "db file should be created");
        // `seaql_migrations` is created by sea-orm-migration's `Migrator::up`;
        // its presence is the canonical "migrations ran" assertion.
        let row = conn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT name FROM sqlite_master WHERE type='table' AND name='seaql_migrations'",
            ))
            .await
            .unwrap();
        let name: Option<String> = row.map(|r| r.try_get::<String>("", "name").unwrap());
        assert_eq!(name.as_deref(), Some("seaql_migrations"));
    }

    #[tokio::test]
    async fn initialize_is_idempotent() {
        // Calling initialize_campaign_storage twice with the same id and
        // data_dir must succeed; the migrator no-ops on already-applied
        // migrations via `seaql_migrations`.
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let campaign_id = CampaignId::generate();
        let (path1, _) = initialize_campaign_storage(&campaign_id, tmp.path())
            .await
            .unwrap();
        let (path2, _) = initialize_campaign_storage(&campaign_id, tmp.path())
            .await
            .unwrap();
        assert_eq!(path1, path2);
    }

    #[tokio::test]
    async fn ping_returns_pong_and_bumps_activity() {
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let campaign_id = CampaignId::generate();
        let (db_path, conn) = initialize_campaign_storage(&campaign_id, tmp.path())
            .await
            .unwrap();
        // Long idle so the timer doesn't fire during the test.
        let args = fast_eviction_args(campaign_id, db_path, conn, 60_000, 60_000);
        let actor_ref = CampaignSupervisor::spawn(args);
        let reply = actor_ref.ask(Ping).await.unwrap();
        assert_eq!(reply, Pong);
        actor_ref.stop_gracefully().await.unwrap();
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
    }

    #[tokio::test]
    async fn set_stop_cause_is_first_writer_wins() {
        // Race coverage: when the supervisor self-tags (e.g. IdleCheck
        // wrote `Idle`) and a drain-side `SetStopCause(Drain)` arrives
        // afterwards, the original tag must stick. The drain message is
        // the new one and the supervisor's own decision is the more
        // accurate cause.
        ensure_vec0();
        let tmp = TempDir::new().unwrap();
        let campaign_id = CampaignId::generate();
        let (db_path, conn) = initialize_campaign_storage(&campaign_id, tmp.path())
            .await
            .unwrap();
        // Long idle so the timer doesn't write a cause out from under us.
        let args = fast_eviction_args(campaign_id, db_path, conn, 60_000, 60_000);
        let actor_ref = CampaignSupervisor::spawn(args);
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
        let campaign_id = CampaignId::generate();
        let (db_path, conn) = initialize_campaign_storage(&campaign_id, tmp.path())
            .await
            .unwrap();
        // Idle threshold 30ms, check every 20ms. After at least 50ms the
        // actor should have evicted itself.
        let args = fast_eviction_args(campaign_id, db_path, conn, 30, 20);
        let actor_ref = CampaignSupervisor::spawn(args);
        // Wait long enough for at least two idle checks past the threshold.
        tokio::time::sleep(Duration::from_millis(200)).await;
        actor_ref.wait_for_shutdown_with_result(|_| ()).await;
        // After shutdown, sends fail.
        assert!(actor_ref.ask(Ping).await.is_err());
    }
}
