use std::path::{Path, PathBuf};

use familiar_systems_app_shared::id::CampaignId;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("object storage error: {0}")]
    ObjectStore(#[from] object_store::Error),
    #[error("campaign not found in storage: {campaign_id}")]
    NotFound { campaign_id: String },
}

#[async_trait::async_trait]
pub trait CampaignStore: Send + Sync + 'static {
    async fn checkout(&self, campaign_id: &CampaignId) -> Result<PathBuf, StoreError>;
    async fn writeback(&self, campaign_id: &CampaignId, path: &Path) -> Result<(), StoreError>;
    async fn release(&self, campaign_id: &CampaignId, path: &Path) -> Result<(), StoreError>;
}
