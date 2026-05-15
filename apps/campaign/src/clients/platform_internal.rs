//! Outbound HTTP client: campaign → platform tier `/internal/platform/*`.
//!
//! Mirror of `apps/platform/src/clients/campaign_internal.rs`. Bearer is
//! pre-installed as a default header so call sites don't have to remember it.

use familiar_systems_app_shared::campaigns::internal::InitFailedRequest;
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

    /// `POST /internal/platform/campaigns/<id>/init-failed` — tells the
    /// platform "I tried to initialize this campaign and failed."  The
    /// platform persists `reason` onto `campaigns.last_init_error`.
    pub async fn report_init_failed(
        &self,
        campaign_id: &str,
        reason: &str,
    ) -> Result<(), PlatformInternalError> {
        let url = format!(
            "{}/internal/platform/campaigns/{}/init-failed",
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
            .and(path("/internal/platform/campaigns/abc/init-failed"))
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
            .and(path("/internal/platform/campaigns/abc/init-failed"))
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
}
