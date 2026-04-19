//! Hanko API wire format.
//!
//! **Wire format, not domain types.** Deserialization and serialization
//! targets that mirror Hanko's HTTP API shape exactly. Private to the `auth`
//! module — these types must never leak to callers outside [`super`]. To move
//! a wire-format value into the domain, use [`super::HankoClaims::try_from`].

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Raw deserialization target for Hanko's session claims.
///
/// Mirrors Hanko's JSON exactly, including nullable email. Use
/// [`super::HankoClaims::try_from`] to normalize and validate.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct HankoClaimsWire {
    pub(super) subject: String,
    pub(super) email: Option<HankoEmailWire>,
    pub(super) expiration: DateTime<Utc>,
    pub(super) session_id: String,
}

/// Raw deserialization target for a Hanko email entry.
///
/// `is_primary` is meaningless under our one-email-per-user config.
/// `is_verified` is checked at `TryFrom` time and then discarded — it does not
/// appear in the domain type.
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
