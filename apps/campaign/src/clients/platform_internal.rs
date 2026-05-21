//! Outbound HTTP client: campaign -> platform tier `/internal/platform/*`.
//!
//! Mirror of `apps/platform/src/clients/campaign_internal.rs`. Bearer is
//! pre-installed as a default header so call sites don't have to remember it.

use familiar_systems_app_shared::campaigns::internal::{
    HeartbeatRequest, InitFailedRequest, PatchCampaignMirror,
};
use familiar_systems_app_shared::id::CampaignId;
use reqwest::{Client, header};
use std::sync::Arc;

#[derive(Debug, thiserror::Error)]
pub enum PlatformInternalError {
    #[error("transport error: {0}")]
    Transport(#[from] reqwest::Error),
    #[error("platform tier returned status {status}")]
    Status { status: reqwest::StatusCode },
}

#[derive(Clone)]
pub struct PlatformInternalClient {
    inner: Arc<Inner>,
}

struct Inner {
    http: Client,
    base_url: String,
}

impl PlatformInternalClient {
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

    /// `PATCH /internal/platform/campaign/{id}`: mirrors changed campaign
    /// metadata onto the platform's routing row. Fires after every
    /// successful PATCH on the public API.
    pub async fn patch_campaign(
        &self,
        campaign_id: &str,
        body: &PatchCampaignMirror,
    ) -> Result<(), PlatformInternalError> {
        let url = format!(
            "{}/internal/platform/campaign/{}",
            self.inner.base_url, campaign_id
        );
        let resp = self.inner.http.patch(&url).json(body).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(PlatformInternalError::Status {
                status: resp.status(),
            })
        }
    }

    /// `POST /internal/platform/campaign/{id}/init-failed`: tells the
    /// platform "I tried to complete the wizard and failed." The platform
    /// persists `reason` onto `campaigns.last_init_error`.
    pub async fn report_init_failed(
        &self,
        campaign_id: &str,
        reason: &str,
    ) -> Result<(), PlatformInternalError> {
        let url = format!(
            "{}/internal/platform/campaign/{}/init-failed",
            self.inner.base_url, campaign_id
        );
        let body = InitFailedRequest {
            reason: reason.to_string(),
        };
        let resp = self.inner.http.post(&url).json(&body).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(PlatformInternalError::Status {
                status: resp.status(),
            })
        }
    }
    /// `POST /internal/platform/heartbeat`: send the list of currently loaded
    /// campaign IDs to the platform. The platform replaces its loaded cache
    /// wholesale on each heartbeat.
    pub async fn heartbeat(&self, campaigns: &[CampaignId]) -> Result<(), PlatformInternalError> {
        let url = format!("{}/internal/platform/heartbeat", self.inner.base_url);
        let body = HeartbeatRequest {
            campaigns: campaigns.to_vec(),
        };
        let resp = self.inner.http.post(&url).json(&body).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(PlatformInternalError::Status {
                status: resp.status(),
            })
        }
    }

    /// `DELETE /internal/platform/campaign/{id}/lease`: notify the platform
    /// that this shard released a campaign (idle eviction). Fire-and-forget;
    /// callers should not block shutdown on the result.
    pub async fn release_lease(&self, campaign_id: &str) -> Result<(), PlatformInternalError> {
        let url = format!(
            "{}/internal/platform/campaign/{}/lease",
            self.inner.base_url, campaign_id
        );
        let resp = self.inner.http.delete(&url).send().await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(PlatformInternalError::Status {
                status: resp.status(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn report_init_failed_sends_bearer_and_reason() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/platform/campaign/abc/init-failed"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = PlatformInternalClient::new(server.uri(), "secret");
        client
            .report_init_failed("abc", "deliberate_thin_slice_failure")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn report_init_failed_returns_status_error_on_5xx() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/platform/campaign/abc/init-failed"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = PlatformInternalClient::new(server.uri(), "secret");
        let err = client.report_init_failed("abc", "boom").await.unwrap_err();
        match err {
            PlatformInternalError::Status { status } => {
                assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
            }
            other => panic!("expected Status error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn patch_campaign_sends_bearer_and_uses_patch() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/internal/platform/campaign/abc"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = PlatformInternalClient::new(server.uri(), "secret");
        let mirror = PatchCampaignMirror {
            name: Some("Test Campaign".into()),
            tagline: None,
            game_system: None,
            content_locale: None,
            wizard_completed_at: None,
        };
        client.patch_campaign("abc", &mirror).await.unwrap();
    }

    #[tokio::test]
    async fn release_lease_sends_bearer_and_uses_delete() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/internal/platform/campaign/abc/lease"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = PlatformInternalClient::new(server.uri(), "secret");
        client.release_lease("abc").await.unwrap();
    }

    #[tokio::test]
    async fn release_lease_returns_status_error_on_5xx() {
        let server = MockServer::start().await;
        Mock::given(method("DELETE"))
            .and(path("/internal/platform/campaign/abc/lease"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let client = PlatformInternalClient::new(server.uri(), "secret");
        let err = client.release_lease("abc").await.unwrap_err();
        match err {
            PlatformInternalError::Status { status } => {
                assert_eq!(status, reqwest::StatusCode::INTERNAL_SERVER_ERROR);
            }
            other => panic!("expected Status error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn heartbeat_sends_bearer_and_campaign_list() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/internal/platform/heartbeat"))
            .and(header("authorization", "Bearer secret"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;

        let client = PlatformInternalClient::new(server.uri(), "secret");
        let campaigns = vec![CampaignId::generate(), CampaignId::generate()];
        client.heartbeat(&campaigns).await.unwrap();
    }
}
