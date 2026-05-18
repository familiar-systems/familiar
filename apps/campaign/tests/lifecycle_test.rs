//! End-to-end lifecycle test: ensure -> drain -> reopen.
//!
//! Exercises the full happy path the production main loop drives: a
//! registry is spawned, several campaigns are ensured (each gets a
//! supervisor, DatabaseActor, on-disk file with migrations), the
//! registry's `BeginDrain` is called (the same path SIGTERM takes in
//! `main.rs`), and we assert each `.db` file is intact and readable.

use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign::{
    actors::registry::{BeginDrain, CampaignRegistry, EnsureCampaign},
    db::{connect, register_sqlite_vec},
};
use kameo::actor::Spawn;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::oneshot;

#[tokio::test]
async fn ensure_drain_and_reopen_three_campaigns() {
    register_sqlite_vec();
    let data_dir = TempDir::new().unwrap();

    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        data_dir.path().to_path_buf(),
        Duration::from_secs(300),
        Duration::from_secs(60),
    ));

    let ids: Vec<CampaignId> = (0..3).map(|_| CampaignId::generate()).collect();
    for id in &ids {
        registry
            .ask(EnsureCampaign {
                campaign_id: id.clone(),
                owner_user_id: UserId::generate(),
            })
            .await
            .expect("ensure should succeed");
    }

    // BeginDrain runs the same code path main.rs uses on SIGTERM: hand
    // the supervisor set off to a tokio task that stops them in parallel,
    // then signal the oneshot when the last child has finished its
    // on_stop (which itself drains the DatabaseActor).
    let (tx, rx) = oneshot::channel();
    registry
        .ask(BeginDrain { completion: tx })
        .await
        .expect("BeginDrain");
    rx.await.expect("drain completion");
    registry.stop_gracefully().await.expect("registry stop");
    registry.wait_for_shutdown_with_result(|_| ()).await;

    // Every campaign's DB file should still exist and be openable. We
    // reopen with sea-orm directly (bypassing the actor system, which is
    // gone) and query `seaql_migrations` to confirm the schema survived
    // the drain. Any unflushed WAL or corruption would surface here.
    for id in &ids {
        let path = data_dir.path().join(format!("{}.db", id.0));
        assert!(path.exists(), "{path:?} should exist after drain");
        let url = format!("sqlite://{}?mode=ro", path.display());
        let conn = connect(&url).await.expect("reopen ro");
        let row = conn
            .query_one(Statement::from_string(
                DatabaseBackend::Sqlite,
                "SELECT COUNT(*) AS n FROM seaql_migrations",
            ))
            .await
            .expect("query");
        let n: i64 = row.unwrap().try_get("", "n").unwrap();
        assert!(n > 0, "expected migrations applied for {}", id.0);
    }
}
