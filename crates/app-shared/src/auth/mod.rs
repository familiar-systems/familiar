//! Authentication types.
//!
//! This module spans three distinct contracts that must not be conflated.
//! Each lives in its own submodule so the boundary is structural, not just a
//! comment convention:
//!
//! 1. **External wire format (Hanko)** — [`wire`]. The shape Hanko sends us
//!    over HTTP. We don't control it. Private to this module; never leaks.
//! 2. **Domain types** — [`domain`]. Our invariant-enforcing view of a
//!    session. [`HankoClaims`] is constructed via `TryFrom<HankoClaimsWire>`,
//!    which rejects sessions that don't satisfy our invariants (one verified
//!    email).
//! 3. **API wire format (ours)** — [`api`]. Shapes we emit to our own clients.
//!    [`MeResponse`] is exported to TypeScript via ts-rs; changes are
//!    breaking changes to the frontend.
//!
//! The parse-don't-validate boundary lives at
//! [`domain::HankoClaims::try_from`]. [`HankoSessionValidator::validate`] is
//! the single production entry point that crosses from (1) to (2).

mod api;
mod domain;
mod wire;

pub use api::MeResponse;
pub use domain::HankoClaims;
use std::time::Duration;
use wire::{ValidatePayloadWire, ValidateResponseWire};

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("missing authorization header")]
    MissingHeader,
    #[error("hanko rejected session: {0}")]
    SessionRejected(String),
    #[error("hanko timed out")]
    UpstreamTimeout,
    #[error("hanko request failed: {0}")]
    UpstreamError(String),
    // #[error("hanko request failed: {0}")]
    // RequestFailed(#[from] reqwest::Error),
}

pub struct HankoSessionValidator {
    client: reqwest::Client,
    api_url: String,
}

impl From<reqwest::Error> for AuthError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::UpstreamTimeout
        } else {
            Self::UpstreamError(err.to_string())
        }
    }
}

impl HankoSessionValidator {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
            api_url: api_url.into(),
        }
    }

    /// Validates a session token with Hanko and returns domain claims.
    ///
    /// Single crossing point from external wire format to domain. Downstream
    /// callers holding a [`HankoClaims`] may trust the invariants documented
    /// on that type.
    pub async fn validate(&self, token: &str) -> Result<HankoClaims, AuthError> {
        let url = format!("{}/sessions/validate", self.api_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&ValidatePayloadWire {
                session_token: token,
            })
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AuthError::SessionRejected(format!(
                "HTTP {}",
                resp.status()
            )));
        }
        let body: ValidateResponseWire = resp.json().await?;
        if !body.is_valid {
            return Err(AuthError::SessionRejected("is_valid=false".into()));
        }
        let wire = body
            .claims
            .ok_or_else(|| AuthError::SessionRejected("no claims".into()))?;
        HankoClaims::try_from(wire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_error_display_is_stable() {
        assert_eq!(
            AuthError::MissingHeader.to_string(),
            "missing authorization header"
        );
    }

    #[tokio::test]
    async fn validate_returns_claims_on_is_valid_true() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let sub = "0195b4a0-0000-7000-8000-000000000001";
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sessions/validate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "is_valid": true,
                "claims": {
                    "subject": sub,
                    "email": {"address": "x@y.com", "is_primary": true, "is_verified": true},
                    "expiration": "2099-01-01T00:00:00Z",
                    "session_id": "sess-1"
                }
            })))
            .expect(1)
            .mount(&srv)
            .await;
        let v = HankoSessionValidator::new(srv.uri());
        let c = v.validate("tok").await.unwrap();
        assert_eq!(c.subject.to_string(), sub);
        assert_eq!(c.email, "x@y.com");
        assert_eq!(c.session_id, "sess-1");
    }

    #[tokio::test]
    async fn validate_rejects_on_http_401() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sessions/validate"))
            .respond_with(ResponseTemplate::new(401))
            .expect(1)
            .mount(&srv)
            .await;
        let v = HankoSessionValidator::new(srv.uri());
        assert!(matches!(
            v.validate("t").await,
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[tokio::test]
    async fn validate_rejects_on_is_valid_false() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sessions/validate"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"is_valid": false})),
            )
            .expect(1)
            .mount(&srv)
            .await;
        let v = HankoSessionValidator::new(srv.uri());
        assert!(matches!(
            v.validate("t").await,
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[tokio::test]
    async fn validate_rejects_when_claims_missing() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sessions/validate"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"is_valid": true})),
            )
            .expect(1)
            .mount(&srv)
            .await;
        let v = HankoSessionValidator::new(srv.uri());
        assert!(matches!(
            v.validate("t").await,
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[tokio::test]
    async fn validate_rejects_when_email_missing_from_claims() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sessions/validate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "is_valid": true,
                "claims": {
                    "subject": "u-1",
                    "expiration": "2099-01-01T00:00:00Z",
                    "session_id": "sess-1"
                }
            })))
            .expect(1)
            .mount(&srv)
            .await;
        let v = HankoSessionValidator::new(srv.uri());
        assert!(matches!(
            v.validate("t").await,
            Err(AuthError::SessionRejected(_))
        ));
    }
}
