use std::path::{Path, PathBuf};

use familiar_systems_app_shared::id::CampaignId;
use object_store::aws::AmazonS3Builder;
use object_store::{ObjectStore, ObjectStoreExt};

use crate::config::S3Config;

use super::store::{CampaignStore, StoreError};

pub struct S3CampaignStore {
    store: Box<dyn ObjectStore>,
    local_cache_dir: PathBuf,
}

impl S3CampaignStore {
    pub fn new(config: &S3Config, local_cache_dir: PathBuf) -> Self {
        let store = AmazonS3Builder::new()
            .with_endpoint(&config.endpoint)
            .with_bucket_name(&config.bucket)
            .with_access_key_id(&config.access_key_id)
            .with_secret_access_key(&config.secret_access_key)
            .with_region("auto")
            .with_virtual_hosted_style_request(false)
            .build()
            .expect("failed to build S3 store");
        Self {
            store: Box::new(store),
            local_cache_dir,
        }
    }

    #[cfg(test)]
    fn with_backend(store: Box<dyn ObjectStore>, local_cache_dir: PathBuf) -> Self {
        Self {
            store,
            local_cache_dir,
        }
    }

    fn remote_path(campaign_id: &CampaignId) -> object_store::path::Path {
        object_store::path::Path::from(format!("campaigns/{}/campaign.db", campaign_id.0))
    }

    fn local_path(&self, campaign_id: &CampaignId) -> PathBuf {
        self.local_cache_dir.join(format!("{}.db", campaign_id.0))
    }
}

#[async_trait::async_trait]
impl CampaignStore for S3CampaignStore {
    async fn checkout(&self, campaign_id: &CampaignId) -> Result<PathBuf, StoreError> {
        tokio::fs::create_dir_all(&self.local_cache_dir).await?;
        let local = self.local_path(campaign_id);
        let remote = Self::remote_path(campaign_id);

        match self.store.get(&remote).await {
            Ok(result) => {
                let bytes = result.bytes().await?;
                tokio::fs::write(&local, &bytes).await?;
                tracing::info!(
                    campaign_id = %campaign_id.0,
                    bytes = bytes.len(),
                    "downloaded campaign database from object storage"
                );
            }
            Err(object_store::Error::NotFound { .. }) => {
                tracing::info!(
                    campaign_id = %campaign_id.0,
                    "no existing database in object storage, will create fresh"
                );
            }
            Err(e) => return Err(StoreError::ObjectStore(e)),
        }

        Ok(local)
    }

    async fn writeback(&self, campaign_id: &CampaignId, path: &Path) -> Result<(), StoreError> {
        let bytes = tokio::fs::read(path).await?;
        let len = bytes.len();
        let remote = Self::remote_path(campaign_id);
        self.store
            .put(&remote, bytes.into())
            .await
            .map_err(StoreError::ObjectStore)?;
        tracing::info!(
            campaign_id = %campaign_id.0,
            bytes = len,
            "wrote campaign database to object storage"
        );
        Ok(())
    }

    async fn release(&self, campaign_id: &CampaignId, path: &Path) -> Result<(), StoreError> {
        self.writeback(campaign_id, path).await?;
        tokio::fs::remove_file(path).await?;
        tracing::info!(
            campaign_id = %campaign_id.0,
            "released local campaign database cache"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object_store::memory::InMemory;
    use tempfile::TempDir;

    fn test_store(tmp: &TempDir) -> S3CampaignStore {
        S3CampaignStore::with_backend(Box::new(InMemory::new()), tmp.path().to_path_buf())
    }

    #[tokio::test]
    async fn checkout_new_campaign_returns_local_path() {
        let tmp = TempDir::new().unwrap();
        let store = test_store(&tmp);
        let id = CampaignId::generate();

        let path = store.checkout(&id).await.unwrap();
        assert_eq!(path, tmp.path().join(format!("{}.db", id.0)));
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn checkout_existing_campaign_downloads() {
        let tmp = TempDir::new().unwrap();
        let store = test_store(&tmp);
        let id = CampaignId::generate();

        let remote = S3CampaignStore::remote_path(&id);
        let payload = b"SQLite format 3\0";
        store
            .store
            .put(&remote, payload.as_ref().into())
            .await
            .unwrap();

        let path = store.checkout(&id).await.unwrap();
        assert!(path.exists());
        assert_eq!(tokio::fs::read(&path).await.unwrap(), payload);
    }

    #[tokio::test]
    async fn writeback_uploads_to_remote() {
        let tmp = TempDir::new().unwrap();
        let store = test_store(&tmp);
        let id = CampaignId::generate();

        let local = tmp.path().join(format!("{}.db", id.0));
        let payload = b"test-database-contents";
        tokio::fs::write(&local, payload).await.unwrap();

        store.writeback(&id, &local).await.unwrap();

        let remote = S3CampaignStore::remote_path(&id);
        let result = store.store.get(&remote).await.unwrap();
        let bytes = result.bytes().await.unwrap();
        assert_eq!(&bytes[..], payload);
    }

    #[tokio::test]
    async fn release_uploads_then_deletes_local() {
        let tmp = TempDir::new().unwrap();
        let store = test_store(&tmp);
        let id = CampaignId::generate();

        let local = tmp.path().join(format!("{}.db", id.0));
        let payload = b"test-database-contents";
        tokio::fs::write(&local, payload).await.unwrap();

        store.release(&id, &local).await.unwrap();

        assert!(!local.exists());

        let remote = S3CampaignStore::remote_path(&id);
        let result = store.store.get(&remote).await.unwrap();
        let bytes = result.bytes().await.unwrap();
        assert_eq!(&bytes[..], payload);
    }

    #[tokio::test]
    async fn checkout_creates_parent_directory() {
        let tmp = TempDir::new().unwrap();
        let nested = tmp.path().join("deeply").join("nested");
        let store = S3CampaignStore::with_backend(Box::new(InMemory::new()), nested.clone());
        let id = CampaignId::generate();

        store.checkout(&id).await.unwrap();
        assert!(nested.exists());
    }
}
