//! Domain types for authentication.
//!
//! **Invariant-enforcing view of a validated Hanko session.** Constructed at
//! the `TryFrom` boundary; downstream code may trust the documented invariants
//! without re-checking. See [`super::wire`] for the raw API shape we consume.

use super::{AuthError, wire::HankoClaimsWire};
use chrono::{DateTime, Utc};
use uuid::Uuid;

/// A validated Hanko session.
///
/// **Domain type.** Constructing one guarantees:
/// - `subject` is a valid UUID (Hanko's tenant-generated user id)
/// - `email` is present, non-empty, and verified by Hanko
/// - the session was accepted by Hanko (`is_valid: true` at validation time)
///
/// Callers holding a `HankoClaims` may assume these invariants without
/// re-checking.
///
/// ## Subject is a UUID
///
/// The OIDC `sub` claim is a string at the wire level, but Hanko's tenant
/// mints UUID-shaped subjects and our `users.id` column is a native UUID.
/// Parsing here means downstream code can treat the subject as a UUID
/// without repeating the parse or handling malformed input.
///
/// ## Email is required
///
/// Hanko's session/user schema allows null/absent email (passkey-only and
/// identifier-only accounts are supported upstream). Our product invariant
/// is that every familiar.systems user has a verified email (for campaign
/// invites, billing, GM notifications, suggestion digests). Enforcing it
/// here means downstream code, including the `users` schema (NOT NULL +
/// UNIQUE on `email`), never has to handle the null case.
///
/// Upstream references:
/// - Session/claims shape: <https://docs.hanko.io/api-reference/public/session-management/validate-a-session>
/// - User object (email nullability): <https://docs.hanko.io/api-reference/public/user-management/get-a-user-by-id>
#[derive(Debug, Clone)]
pub struct HankoClaims {
    pub subject: Uuid,
    pub email: String,
    pub expiration: DateTime<Utc>,
    pub session_id: String,
}

impl TryFrom<HankoClaimsWire> for HankoClaims {
    type Error = AuthError;

    /// Validates Hanko's wire-format claims and projects them to the domain.
    ///
    /// Rejects with [`AuthError::SessionRejected`] if the wire payload violates
    /// our invariants: non-UUID subject, missing email, or email marked
    /// `is_verified: false`.
    fn try_from(w: HankoClaimsWire) -> Result<Self, Self::Error> {
        let subject = Uuid::parse_str(&w.subject)
            .map_err(|_| AuthError::SessionRejected("subject is not a UUID".into()))?;
        let email = w
            .email
            .ok_or_else(|| AuthError::SessionRejected("claims missing required email".into()))?;
        if !email.is_verified {
            return Err(AuthError::SessionRejected("email not verified".into()));
        }
        Ok(HankoClaims {
            subject,
            email: email.address,
            expiration: w.expiration,
            session_id: w.session_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SUB: &str = "0195b4a0-0000-7000-8000-000000000001";

    #[test]
    fn wire_with_verified_email_converts_to_domain() {
        let raw = format!(
            r#"{{"subject":"{TEST_SUB}","email":{{"address":"a@b.com","is_primary":true,"is_verified":true}},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}}"#
        );
        let w: HankoClaimsWire = serde_json::from_str(&raw).unwrap();
        let c: HankoClaims = w.try_into().unwrap();
        assert_eq!(c.subject.to_string(), TEST_SUB);
        assert_eq!(c.email, "a@b.com");
        assert_eq!(c.session_id, "s1");
    }

    #[test]
    fn non_uuid_subject_is_rejected() {
        let raw = r#"{"subject":"not-a-uuid","email":{"address":"a@b.com","is_primary":true,"is_verified":true},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}"#;
        let w: HankoClaimsWire = serde_json::from_str(raw).unwrap();
        assert!(matches!(
            HankoClaims::try_from(w),
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[test]
    fn null_email_is_rejected() {
        let raw = format!(
            r#"{{"subject":"{TEST_SUB}","email":null,"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}}"#
        );
        let w: HankoClaimsWire = serde_json::from_str(&raw).unwrap();
        assert!(matches!(
            HankoClaims::try_from(w),
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[test]
    fn absent_email_is_rejected() {
        let raw = format!(
            r#"{{"subject":"{TEST_SUB}","expiration":"2099-12-31T00:00:00Z","session_id":"s1"}}"#
        );
        let w: HankoClaimsWire = serde_json::from_str(&raw).unwrap();
        assert!(matches!(
            HankoClaims::try_from(w),
            Err(AuthError::SessionRejected(_))
        ));
    }

    #[test]
    fn unverified_email_is_rejected() {
        let raw = format!(
            r#"{{"subject":"{TEST_SUB}","email":{{"address":"a@b.com","is_primary":true,"is_verified":false}},"expiration":"2099-12-31T00:00:00Z","session_id":"s1"}}"#
        );
        let w: HankoClaimsWire = serde_json::from_str(&raw).unwrap();
        assert!(matches!(
            HankoClaims::try_from(w),
            Err(AuthError::SessionRejected(_))
        ));
    }
}
