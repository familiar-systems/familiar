//! Domain types for authentication.
//!
//! **Invariant-enforcing view of a validated Hanko session.** Constructed at
//! the `TryFrom` boundary; downstream code may trust the documented invariants
//! without re-checking. See [`super::wire`] for the raw API shape we consume.

use super::{AuthError, wire::HankoClaimsWire};
use chrono::{DateTime, Utc};

/// A validated Hanko session.
///
/// **Domain type.** Constructing one guarantees:
/// - `email` is present, non-empty, and verified by Hanko
/// - the session was accepted by Hanko (`is_valid: true` at validation time)
///
/// Callers holding a `HankoClaims` may assume these invariants without
/// re-checking.
#[derive(Debug, Clone)]
pub struct HankoClaims {
    pub subject: String,
    pub email: String,
    pub expiration: DateTime<Utc>,
    pub session_id: String,
}

impl TryFrom<HankoClaimsWire> for HankoClaims {
    type Error = AuthError;

    /// Validates Hanko's wire-format claims and projects them to the domain.
    ///
    /// Rejects with [`AuthError::SessionRejected`] if the wire payload violates
    /// our invariants: missing email, or email marked `is_verified: false`.
    fn try_from(w: HankoClaimsWire) -> Result<Self, Self::Error> {
        let email = w
            .email
            .ok_or_else(|| AuthError::SessionRejected("claims missing required email".into()))?;
        if !email.is_verified {
            return Err(AuthError::SessionRejected("email not verified".into()));
        }
        Ok(HankoClaims {
            subject: w.subject,
            email: email.address,
            expiration: w.expiration,
            session_id: w.session_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_with_verified_email_converts_to_domain() {
        let raw = r#"{"subject":"sub-1","email":{"address":"a@b.com","is_primary":true,"is_verified":true},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let w: HankoClaimsWire = serde_json::from_str(raw).unwrap();
        let c: HankoClaims = w.try_into().unwrap();
        assert_eq!(c.subject, "sub-1");
        assert_eq!(c.email, "a@b.com");
        assert_eq!(c.session_id, "s1");
    }

    #[test]
    fn null_email_is_rejected() {
        let raw = r#"{"subject":"sub-1","email":null,"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let w: HankoClaimsWire = serde_json::from_str(raw).unwrap();
        assert!(matches!(
            HankoClaims::try_from(w),
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[test]
    fn absent_email_is_rejected() {
        let raw = r#"{"subject":"sub-1","expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let w: HankoClaimsWire = serde_json::from_str(raw).unwrap();
        assert!(matches!(
            HankoClaims::try_from(w),
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[test]
    fn unverified_email_is_rejected() {
        let raw = r#"{"subject":"sub-1","email":{"address":"a@b.com","is_primary":true,"is_verified":false},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let w: HankoClaimsWire = serde_json::from_str(raw).unwrap();
        assert!(matches!(
            HankoClaims::try_from(w),
            Err(AuthError::SessionRejected(_))
        ));
    }
}
