use std::path::{Path, PathBuf};

use familiar_systems_app_shared::id::CampaignId;

use super::store::{CampaignStore, StoreError};

pub struct LocalCampaignStore {
    data_dir: PathBuf,
}

impl LocalCampaignStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }
}

#[async_trait::async_trait]
impl CampaignStore for LocalCampaignStore {
    async fn checkout(&self, campaign_id: &CampaignId) -> Result<PathBuf, StoreError> {
        tokio::fs::create_dir_all(&self.data_dir).await?;
        Ok(self.data_dir.join(format!("{}.db", campaign_id.0)))
    }

    async fn writeback(&self, _campaign_id: &CampaignId, _path: &Path) -> Result<(), StoreError> {
        Ok(())
    }

    async fn release(&self, _campaign_id: &CampaignId, _path: &Path) -> Result<(), StoreError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn checkout_returns_correct_path() {
        let tmp = TempDir::new().unwrap();
        let store = LocalCampaignStore::new(tmp.path().to_path_buf());
        let id = CampaignId::generate();
        let path = store.checkout(&id).await.unwrap();
        assert_eq!(path, tmp.path().join(format!("{}.db", id.0)));
    }

    #[tokio::test]
    async fn checkout_creates_parent_directory() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("deeply").join("nested");
        let store = LocalCampaignStore::new(nested.clone());
        let id = CampaignId::generate();
        store.checkout(&id).await.unwrap();
        assert!(nested.exists());
    }

    #[tokio::test]
    async fn writeback_is_noop() {
        let tmp = TempDir::new().unwrap();
        let store = LocalCampaignStore::new(tmp.path().to_path_buf());
        let id = CampaignId::generate();
        let path = tmp.path().join("dummy.db");
        store.writeback(&id, &path).await.unwrap();
    }

    #[tokio::test]
    async fn release_is_noop() {
        let tmp = TempDir::new().unwrap();
        let store = LocalCampaignStore::new(tmp.path().to_path_buf());
        let id = CampaignId::generate();
        let path = tmp.path().join("dummy.db");
        store.release(&id, &path).await.unwrap();
    }
}
