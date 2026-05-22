//! Pluggable storage backend for campaign database files.
//!
//! [`CampaignStore`] abstracts where campaign SQLite files physically reside. The
//! [`CampaignSupervisor`](crate::actors::supervisor::CampaignSupervisor) owns the store;
//! [`CampaignDatabase`](super::database::CampaignDatabase) composes it during checkout and
//! release. See the [module glossary](super) for term definitions.

use std::path::{Path, PathBuf};

use familiar_systems_app_shared::id::CampaignId;

/// Errors from storage lifecycle operations (checkout, writeback, release).
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("object storage error: {0}")]
    ObjectStore(#[from] object_store::Error),
    #[error("campaign not found in storage: {campaign_id}")]
    NotFound { campaign_id: String },
}

/// Abstracts where campaign SQLite files physically reside.
///
/// Two implementations exist:
/// - [`LocalCampaignStore`](super::store_local::LocalCampaignStore): files live on the local
///   filesystem. Used in development and the planned self-hosted deployment mode.
/// - [`S3CampaignStore`](super::store_s3::S3CampaignStore): files live in S3-compatible object
///   storage with a local cache directory. Used in hosted/managed deployments.
#[async_trait::async_trait]
pub trait CampaignStore: Send + Sync + 'static {
    /// Acquire a local path to a campaign's database file.
    ///
    /// For S3 storage, downloads the file from object storage to the local cache directory.
    /// For local storage, returns the path on disk (creating parent directories if needed).
    ///
    /// If no remote file exists yet (new campaign), returns the local path without error.
    /// The database layer creates the file when it opens the connection with `mode=rwc`.
    async fn checkout(&self, campaign_id: &CampaignId) -> Result<PathBuf, StoreError>;

    /// Upload the local campaign file to the remote store for durability.
    ///
    /// Called periodically (~30s) by the supervisor during active use so that crash recovery
    /// loses at most one writeback interval of edits. For local storage, this is a no-op
    /// since the local file is already the source of truth.
    async fn writeback(&self, campaign_id: &CampaignId, path: &Path) -> Result<(), StoreError>;

    /// Final writeback followed by cleanup of the local cache copy.
    ///
    /// Called when a campaign is no longer needed on this shard (idle eviction, shutdown
    /// drain, or platform-initiated release). For local storage, this is a no-op since
    /// there is no remote store to sync and no cache to clean.
    async fn release(&self, campaign_id: &CampaignId, path: &Path) -> Result<(), StoreError>;
}
