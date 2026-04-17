use chrono::{DateTime, Utc};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct HankoClaims {
    pub subject: String,
    pub email: Option<HankoEmail>,
    pub expiration: DateTime<Utc>,
    pub session_id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HankoEmail {
    pub address: String,
    pub is_primary: bool,
    pub is_verified: bool,
}

#[derive(thiserror::Error, Debug)]
pub enum AuthError {
    #[error("missing authorization header")]
    MissingHeader,
    #[error("hanko rejected session: {0}")]
    SessionRejected(String),
    #[error("hanko request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
}

pub struct HankoSessionValidator {
    client: reqwest::Client,
    api_url: String,
}

#[derive(serde::Deserialize)]
struct ValidateResponse {
    is_valid: bool,
    claims: Option<HankoClaims>,
}

#[derive(serde::Serialize)]
struct ValidatePayload<'a> {
    session_token: &'a str,
}

impl HankoSessionValidator {
    pub fn new(api_url: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_url: api_url.into(),
        }
    }

    pub async fn validate(&self, token: &str) -> Result<HankoClaims, AuthError> {
        let url = format!("{}/sessions/validate", self.api_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .json(&ValidatePayload { session_token: token })
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(AuthError::SessionRejected(format!("HTTP {}", resp.status())));
        }
        let body: ValidateResponse = resp.json().await?;
        if !body.is_valid {
            return Err(AuthError::SessionRejected("is_valid=false".into()));
        }
        body.claims
            .ok_or_else(|| AuthError::SessionRejected("no claims".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_deserialize_from_hanko_response_shape() {
        let raw = r#"{"subject":"sub-1","email":{"address":"a@b.com","is_primary":true,"is_verified":true},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert_eq!(c.subject, "sub-1");
        let email = c.email.unwrap();
        assert_eq!(email.address, "a@b.com");
        assert!(email.is_primary);
        assert!(email.is_verified);
    }

    #[test]
    fn claims_deserialize_with_null_email() {
        let raw = r#"{"subject":"sub-1","email":null,"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert!(c.email.is_none());
    }

    #[test]
    fn claims_deserialize_with_absent_email() {
        let raw = r#"{"subject":"sub-1","expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert!(c.email.is_none());
    }

    #[test]
    fn auth_error_display_is_stable() {
        assert_eq!(AuthError::MissingHeader.to_string(), "missing authorization header");
    }

    #[tokio::test]
    async fn validate_returns_claims_on_is_valid_true() {
        use wiremock::{
            matchers::{method, path},
            Mock, MockServer, ResponseTemplate,
        };
        let srv = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/sessions/validate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "is_valid": true,
                "claims": {
                    "subject": "u-1",
                    "email": {"address": "x@y.com", "is_primary": true, "is_verified": true},
                    "expiration": "2099-01-01T00:00:00Z",
                    "session_id": "sess-1"
                }
            })))
            .mount(&srv)
            .await;
        let v = HankoSessionValidator::new(srv.uri());
        let c = v.validate("tok").await.unwrap();
        assert_eq!(c.subject, "u-1");
        assert_eq!(c.session_id, "sess-1");
    }
}
