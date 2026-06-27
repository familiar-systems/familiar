//! Typed errors for the campaign binary.
//!
//! `StartupError` is what `main` returns; failure here means the process
//! cannot serve. `InitError` is a per-campaign error surfaced through the
//! supervisor's spawn path. `EnsureError` is what the registry's
//! `EnsureCampaign` handler returns to HTTP handlers.

use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::StatusCode;
use familiar_systems_app_shared::id::CampaignId;

use crate::persistence::StoreError;

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
    #[error("failed to check out campaign {campaign_id} from storage: {source}")]
    Checkout {
        campaign_id: CampaignId,
        #[source]
        source: StoreError,
    },
    #[error("failed to release campaign {campaign_id} to storage: {source}")]
    Release {
        campaign_id: CampaignId,
        #[source]
        source: StoreError,
    },
}

/// Error returned by `CampaignRegistry::EnsureCampaign` to HTTP handlers.
#[derive(Debug, thiserror::Error)]
pub enum EnsureError {
    #[error("init failed: {0}")]
    Init(#[from] InitError),
    /// The registry is in its drain phase and won't accept new
    /// campaigns. Maps to 503 at the HTTP layer; the platform retries
    /// against whichever shard takes over the campaign.
    #[error("registry is shutting down")]
    ShuttingDown,
}

/// Outcome of resolving a campaign from the registry's routing table to a
/// live [`CampaignHandle`](crate::actors::registry::CampaignHandle). Checkout
/// is async: a request may land while a campaign is mid-load or mid-drain.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// No entry in the routing table. The campaign isn't checked out on this
    /// shard (the platform hasn't leased it here).
    #[error("campaign not loaded on this shard")]
    NotLoaded,
    /// The campaign is being torn down (platform release or shard drain).
    #[error("campaign is draining")]
    Draining,
    /// The async load reached a terminal failure (init error, supervisor
    /// died during startup). Transient; a retry re-attempts checkout.
    #[error("campaign load failed")]
    LoadFailed,
    /// Still loading when the bounded wait elapsed. Transient; the load
    /// continues in the background, so a retry usually finds it ready.
    #[error("campaign still loading")]
    StillLoading,
}

impl ResolveError {
    /// Default HTTP status. `NotLoaded` is the only 404 (the campaign genuinely
    /// isn't here); every other variant is a transient 503 the caller retries.
    /// Callers that need a different mapping (e.g. internal create treating a
    /// load failure as 500) match the variant directly.
    pub fn status(&self) -> StatusCode {
        match self {
            ResolveError::NotLoaded => StatusCode::NOT_FOUND,
            ResolveError::Draining | ResolveError::LoadFailed | ResolveError::StillLoading => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    }
}
