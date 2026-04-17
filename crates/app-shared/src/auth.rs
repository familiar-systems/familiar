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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_deserialize_from_hanko_response_shape() {
        let raw = r#"{"subject":"sub-1","email":{"address":"a@b.com","is_verified":true},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert_eq!(c.subject, "sub-1");
        assert!(c.email.unwrap().is_verified);
    }

    #[test]
    fn claims_deserialize_with_null_email() {
        let raw = r#"{"subject":"sub-1","email":null,"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let c: HankoClaims = serde_json::from_str(raw).unwrap();
        assert!(c.email.is_none());
    }

    #[test]
    fn auth_error_display_is_stable() {
        assert_eq!(AuthError::MissingHeader.to_string(), "missing authorization header");
    }
}
