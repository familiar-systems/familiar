//! Outbound HTTP client: platform -> campaign tier `/internal/campaign/*`.
//!
//! One client per process; cloned cheaply per request. Bearer header is
//! attached on every send via the [`reqwest::Client`] default headers, so
//! call sites don't have to remember it.

use familiar_systems_app_shared::campaigns::internal::CreateCampaignRequest;
use familiar_systems_app_shared::id::{CampaignId, UserId};
use reqwest::{Client, header};
use std::sync::Arc;

/// Errors surfaced from outbound campaign-internal calls.
///
/// Kept distinct from [`crate::error::AppError`] so route handlers can
/// decide whether to map a failure into a 5xx (the create-flow case: the
/// SPA's retry handles it) or treat it as a soft failure (the metadata-mirror
/// case: a missed mirror should not break the user-visible response).
#[derive(Debug, thiserror::Error)]
pub enum CampaignInternalError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("campaign tier returned status {status}")]
    Status { status: reqwest::StatusCode },
}

#[derive(Clone)]
pub struct CampaignInternalClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: Client,
    base_url: String,
}

impl CampaignInternalClient {
    /// Build a client targeting `base_url` (e.g. `http://localhost:3001`)
    /// with the bearer token pre-installed as a default header. The
    /// `Authorization` value is constructed once at startup; subsequent
    /// requests pay no per-call header allocation.
    pub fn new(base_url: impl Into<String>, bearer: &str) -> Self {
        let mut headers = header::HeaderMap::new();
        let mut auth = header::HeaderValue::from_str(&format!("Bearer {bearer}"))
            .expect("bearer must be ASCII");
        auth.set_sensitive(true);
        headers.insert(header::AUTHORIZATION, auth);

        let http = Client::builder()
            .default_headers(headers)
            .build()
            .expect("reqwest client build");

        Self {
            inner: Arc::new(Inner {
                http,
                base_url: base_url.into(),
            }),
        }
    }

    /// `POST /internal/campaign`: create a new campaign on the shard with
    /// the given owner. Idempotent on `campaign_id`.
    pub async fn create_campaign(
        &self,
        campaign_id: &CampaignId,
        owner_user_id: &UserId,
    ) -> Result<(), CampaignInternalError> {
        let url = format!("{}/internal/campaign", self.inner.base_url);
        let body = CreateCampaignRequest {
            campaign_id: campaign_id.clone(),
            owner_user_id: owner_user_id.clone(),
        };
        let resp = self.inner.http.post(&url).json(&body).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(CampaignInternalError::Status {
                status: resp.status(),
            })
        }
    }

    /// `PUT /internal/campaign/{id}/lease`: ensure the campaign is loaded
    /// on the shard. For cold checkouts; no body needed.
    pub async fn acquire_lease(
        &self,
        campaign_id: &CampaignId,
    ) -> Result<(), CampaignInternalError> {
        let url = format!(
            "{}/internal/campaign/{}/lease",
            self.inner.base_url, campaign_id.0
        );
        let resp = self.inner.http.put(&url).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(CampaignInternalError::Status {
                status: resp.status(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fs_id::Nanoid;
    use uuid::Uuid;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn campaign_id() -> CampaignId {
        CampaignId(Nanoid("test-id".to_string()))
    }

    fn user_id() -> UserId {
        UserId(Uuid::now_v7())
    }

    #[tokio::test]
    async fn create_campaign_sends_bearer_and_returns_ok() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/campaign"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = CampaignInternalClient::new(server.uri(), "secret");
        client
            .create_campaign(&campaign_id(), &user_id())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn create_campaign_returns_status_error_on_5xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/campaign"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = CampaignInternalClient::new(server.uri(), "secret");
        let err = client
            .create_campaign(&campaign_id(), &user_id())
            .await
            .unwrap_err();
        match err {
            CampaignInternalError::Status { status } => {
                assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
            }
            other => panic!("expected Status error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn acquire_lease_uses_put_with_no_body() {
        let server = MockServer::start().await;
        Mock::given(method("PUT"))
            .and(path("/internal/campaign/test-id/lease"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = CampaignInternalClient::new(server.uri(), "secret");
        client.acquire_lease(&campaign_id()).await.unwrap();
    }
}
