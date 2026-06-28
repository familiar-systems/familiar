//! End-to-end lifecycle test: ensure -> drain -> reopen.
//!
//! Exercises the full happy path the production main loop drives: a
//! registry is spawned, several campaigns are ensured (each gets a
//! supervisor, DatabaseWriteActor, on-disk file with migrations), the
//! registry's `BeginDrain` is called (the same path SIGTERM takes in
//! `main.rs`), and we assert each `.db` file is intact and readable.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use familiar_systems_campaign::{
    actors::registry::{BeginDrain, CampaignRegistry, CampaignTable, CreateCampaign, resolve},
    db::{connect, register_sqlite_vec},
    persistence::LocalCampaignStore,
};
use kameo::actor::Spawn;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use tempfile::TempDir;
use tokio::sync::oneshot;

#[tokio::test]
async fn ensure_drain_and_reopen_three_campaigns() {
    register_sqlite_vec();
    let data_dir = TempDir::new().unwrap();
    let store: Arc<dyn familiar_systems_campaign::persistence::CampaignStore> =
        Arc::new(LocalCampaignStore::new(data_dir.path().to_path_buf()));

    let table: CampaignTable = Arc::new(ArcSwap::from_pointee(HashMap::new()));
    let registry = CampaignRegistry::spawn(CampaignRegistry::new(
        table,
        store,
        Duration::from_secs(300),
        Duration::from_secs(60),
        None,
    ));

    let ids: Vec<CampaignId> = (0..3).map(|_| CampaignId::generate()).collect();
    for id in &ids {
        let state = registry
            .ask(CreateCampaign {
                campaign_id: id.clone(),
                owner_user_id: UserId::generate(),
            })
            .await
            .expect("ensure should succeed");
        // Checkout is async; await readiness so the supervisor's on_start
        // (DB create + migrate) has run before we drain and inspect the file.
        resolve(Some(state), Duration::from_secs(30))
            .await
            .expect("campaign should reach ready");
    }

    let (tx, rx) = oneshot::channel();
    registry
        .ask(BeginDrain { completion: tx })
        .await
        .expect("BeginDrain");
    rx.await.expect("drain completion");
    registry.stop_gracefully().await.expect("registry stop");
    registry.wait_for_shutdown_with_result(|_| ()).await;

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
