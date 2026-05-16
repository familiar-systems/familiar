//! Typed errors for the campaign binary.
//!
//! `StartupError` is what `main` returns; failure here means the process
//! cannot serve. `InitError` is a per-campaign error surfaced through the
//! supervisor's spawn path. `EnsureError` is what the registry's
//! `EnsureCampaign` handler returns to HTTP handlers.

use std::net::SocketAddr;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    /// `Catalog::load_from_embedded` returns `Result<_, String>` (parsed
    /// at build-embed time), so we carry the message verbatim. If that
    /// type ever grows a typed error, swap this for `#[from]`.
    #[error("starter content failed to parse at startup: {0}")]
    Catalog(String),
    #[error("failed to bind {addr}: {source}")]
    Bind {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },
    #[error("HTTP server error: {0}")]
    Serve(#[source] std::io::Error),
}

/// A per-campaign initialization failure. Returned from
/// `CampaignSupervisor::on_start`; the spawner observes this through
/// kameo's spawn-result path and forwards it as `EnsureError::Init`.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("failed to create data directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to open campaign database at {path}: {source}")]
    OpenDatabase {
        path: PathBuf,
        #[source]
        source: sqlx::Error,
    },
    #[error("migration failed: {0}")]
    Migration(#[source] sea_orm::DbErr),
}

/// Error returned by `CampaignRegistry::EnsureCampaign` to HTTP handlers.
#[derive(Debug, thiserror::Error)]
pub enum EnsureError {
    #[error("init failed: {0}")]
    Init(#[from] InitError),
    /// The supervisor was spawned but its actor task ended before
    /// reporting Ready. Treat as a transient failure; the platform may
    /// retry.
    #[error("supervisor died before reaching Ready")]
    SupervisorDied,
    /// The registry is in its drain phase and won't accept new
    /// campaigns. Maps to 503 at the HTTP layer; the platform retries
    /// against whichever shard takes over the campaign.
    #[error("registry is shutting down")]
    ShuttingDown,
}
