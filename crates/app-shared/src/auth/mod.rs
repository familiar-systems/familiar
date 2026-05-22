//! Authentication types.
//!
//! Each submodule owns a distinct contract:
//!
//! - **`wire`** - External wire format (Hanko). The shape Hanko sends us
//!   over HTTP. We don't control it. Private to this module; never leaks.
//! - **`domain`** - Our invariant-enforcing view of a session.
//!   [`HankoClaims`] is constructed via `TryFrom<HankoClaimsWire>`, which
//!   rejects sessions that don't satisfy our invariants (one verified email).
//! - **`validator`** - [`HankoSessionValidator`] and [`AuthError`]. The
//!   single crossing point from external wire format to domain.
//! - **`extractor`** - Axum [`AuthenticatedUser`] extractor, shared by both
//!   binaries.
//! - **`api`** - Shapes we emit to our own clients. Exported to TypeScript
//!   via ts-rs; changes are breaking changes to the frontend.
//!
//! The parse-don't-validate boundary lives at `HankoClaims::try_from`.
//! [`HankoSessionValidator::validate`] is the single production entry point
//! that crosses from wire to domain.

mod api;
mod domain;
pub mod extractor;
mod validator;
mod wire;

pub use api::MeResponse;
pub use domain::HankoClaims;
pub use extractor::AuthenticatedUser;
pub use validator::{AuthError, HankoSessionValidator};
