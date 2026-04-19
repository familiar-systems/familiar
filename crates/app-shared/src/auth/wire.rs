//! Hanko API wire format.
//!
//! **Wire format, not domain types.** Deserialization and serialization
//! targets that mirror Hanko's HTTP API shape exactly. Private to the `auth`
//! module; these types must never leak to callers outside [`super`]. To move
//! a wire-format value into the domain, use [`super::HankoClaims::try_from`].
//!
//! ## Upstream reference
//!
//! Vendor docs for the shapes below:
//! - `POST /sessions/validate` request + response:
//!   <https://docs.hanko.io/api-reference/public/session-management/validate-a-session>
//! - User object schema (source of truth for field nullability / shape):
//!   <https://docs.hanko.io/api-reference/public/user-management/get-a-user-by-id>

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Raw deserialization target for Hanko's session claims.
///
/// Mirrors Hanko's JSON exactly, including nullable email. Hanko's user model
/// allows users without an email (e.g. passkey-only or identifier-only
/// accounts), so the wire type must tolerate `"email": null`. Our domain
/// rejects that case; see [`super::HankoClaims::try_from`].
///
/// Upstream: <https://docs.hanko.io/api-reference/public/session-management/validate-a-session>
#[derive(Debug, Clone, Deserialize)]
pub(super) struct HankoClaimsWire {
    pub(super) subject: String,
    pub(super) email: Option<HankoEmailWire>,
    pub(super) expiration: DateTime<Utc>,
    pub(super) session_id: String,
}

/// Raw deserialization target for a Hanko email entry.
///
/// ## Hanko supports multi-email; we've disabled it
///
/// Hanko models users with *multiple* email addresses by default: add,
/// delete, and mark-as-primary are first-class operations in the API
/// (<https://docs.hanko.io/api-reference/public/email-management/get-a-list-of-emails-of-the-current-user>).
/// We've configured our Hanko tenant so each familiar.systems user has
/// exactly one email. That keeps the domain model simple (one email
/// column per user, no primary-selection UI, no email-array in session
/// claims) and matches the product's notion of "email" as the user's
/// identity for invites, billing, and notifications.
///
/// Consequence: `is_primary` is always `true` in practice and carries no
/// information. It's kept on the wire type for schema fidelity to Hanko's
/// response (a silent drift in Hanko's shape would fail to deserialize),
/// but it's never read downstream.
///
/// `is_verified` is checked at `TryFrom` time in the domain module and
/// then discarded; it does not appear in the domain type.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct HankoEmailWire {
    pub(super) address: String,
    #[allow(dead_code)]
    pub(super) is_primary: bool,
    pub(super) is_verified: bool,
}

/// Raw deserialization target for Hanko's `/sessions/validate` response body.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct ValidateResponseWire {
    pub(super) is_valid: bool,
    pub(super) claims: Option<HankoClaimsWire>,
}

/// Serialization target for the body we POST to Hanko's `/sessions/validate`.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ValidatePayloadWire<'a> {
    pub(super) session_token: &'a str,
}
