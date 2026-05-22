//! Campaign database file lifecycle: checkout, active use, and release.
//!
//! Each campaign is an isolated SQLite file (with sqlite-vec for vector search and sea-orm as
//! the ORM). This module owns the full lifecycle of those files across two layers:
//!
//! - [`CampaignStore`] handles *where* files live: local filesystem for development, S3-compatible
//!   object storage for hosted deployments. See [`LocalCampaignStore`] and [`S3CampaignStore`].
//! - [`CampaignDatabase`] handles *what happens with them*: opening read/write connections, running
//!   migrations, spawning the [`DatabaseActor`](crate::actors::database_writer::DatabaseActor) for
//!   serialized writes, and tearing down cleanly on release.
//!
//! ## Glossary
//!
//! These terms appear throughout the campaign server codebase:
//!
//! - **Checkout**: acquire a campaign's SQLite file for local use. For hosted deployments, this
//!   downloads the file from object storage. For local dev, it resolves a path on disk.
//! - **Writeback**: upload the local campaign file to object storage for durability. Called
//!   periodically (~30s) during active use. A no-op for local storage.
//! - **Release**: final writeback followed by local cache cleanup. Called on idle eviction or
//!   shutdown drain.
//! - **Lease**: platform-level mechanism ensuring a campaign is loaded on exactly one shard at a
//!   time. Acquired by the platform calling `PUT /internal/campaign/{id}/lease`; released on idle
//!   eviction or via `DELETE /internal/campaign/{id}/lease`.
//! - **Drain**: graceful shutdown sequence. The registry stops accepting new campaigns, each
//!   supervisor snapshots its state and releases its campaign database.
//! - **Eviction**: idle-timeout-driven removal of a campaign from memory after persisting its
//!   state. Configured via `CAMPAIGN_IDLE_TIMEOUT_SECS`.

pub mod database;
pub mod store;
pub mod store_local;
pub mod store_s3;

pub use database::{CampaignDatabase, store_from_config};
pub use store::{CampaignStore, StoreError};
pub use store_local::LocalCampaignStore;
pub use store_s3::S3CampaignStore;
