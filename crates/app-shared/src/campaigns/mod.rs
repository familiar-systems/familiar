//! Campaign-related types that cross the platform/campaign boundary.
//!
//! Two surfaces:
//! - [`api`]: wire types we emit to or accept from the SPA. Exported to
//!   TypeScript via ts-rs.
//! - [`internal`]: wire types for platform↔campaign internal HTTP calls.
//!   Rust-only; not exported to TS.
//!
//! Naming convention for internal routes: `/internal/<owner>/...` where
//! `<owner>` names the tier that serves the route (see the bearer middleware
//! at `apps/{platform,campaign}/src/middleware/internal_auth.rs`). The wire
//! types here are caller-and-callee shared; the route owner is documented
//! per-type.

pub mod api;
pub mod internal;
