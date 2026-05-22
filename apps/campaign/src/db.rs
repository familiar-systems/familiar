use sea_orm::{DatabaseConnection, SqlxSqliteConnector};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use std::str::FromStr;
use std::sync::Once;

/// Register sqlite-vec as an auto-extension for this process. Idempotent.
///
/// This is process-global state: every SQLite connection opened in the
/// process from this point on (by sqlx, by anything else) will load vec0
/// automatically. That's by design; we want vec0 available everywhere
/// without per-connection plumbing.
///
/// Call once at process startup (before any `connect()`). The official
/// sqlite-vec demo does the same thing at the top of `main`. We keep it
/// separate from `connect()` so opening a connection doesn't carry a hidden
/// global side effect, and tests that intentionally want raw SQLite (no
/// vec0) can skip it.
pub fn register_sqlite_vec() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // sqlite-vec exposes its init function with its own (structurally
        // identical, nominally distinct) bindings for sqlite3 and
        // sqlite3_api_routines. Transmute to the libsqlite3-sys-typed function
        // pointer that sqlite3_auto_extension expects.
        type InitFn = unsafe extern "C" fn(
            *mut libsqlite3_sys::sqlite3,
            *mut *mut std::os::raw::c_char,
            *const libsqlite3_sys::sqlite3_api_routines,
        ) -> std::os::raw::c_int;

        unsafe {
            let init: InitFn = std::mem::transmute(sqlite_vec::sqlite3_vec_init as *const ());
            libsqlite3_sys::sqlite3_auto_extension(Some(init));
        }
    });
}

/// Open a read-write sqlx sqlite pool and wrap it as a sea-orm `DatabaseConnection`.
///
/// Used by the [`DatabaseActor`](crate::actors::database_writer::DatabaseActor), which
/// serializes writes through its kameo mailbox. A small pool (2 connections) is sufficient
/// since concurrent writes are mailbox-serialized.
///
/// Does **not** register sqlite-vec; that's `register_sqlite_vec`'s job.
/// If you want vec0 available in this connection, the caller registers it
/// before opening anything.
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, sqlx::Error> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(2)
        .connect_with(opts)
        .await?;

    Ok(SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}

/// Open a read-only connection pool against an existing database file.
///
/// WAL mode allows concurrent readers alongside the single writer owned
/// by the [`DatabaseActor`](crate::actors::database_writer::DatabaseActor). The pool is
/// `Clone + Send + Sync` so it can be handed to every actor that needs read access.
///
/// The pool is sized for campaign-scale concurrency: dozens of room actors restoring
/// or querying simultaneously during startup fan-out and AI context-building passes.
/// SQLite WAL readers are cheap (shared read lock, no blocking), so the pool limit
/// governs how many sqlx connections are held open, not how many reads can proceed.
pub async fn connect_readonly(path: &std::path::Path) -> Result<DatabaseConnection, sqlx::Error> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .read_only(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(16)
        .connect_with(opts)
        .await?;

    Ok(SqlxSqliteConnector::from_sqlx_sqlite_pool(pool))
}
