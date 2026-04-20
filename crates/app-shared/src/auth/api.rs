//! API response types (our wire format).
//!
//! **Wire format we own.** Shapes emitted to our own clients (SPA, public
//! site). Distinct from [`super::wire`], which captures Hanko's shape that we
//! merely consume. Changes here are breaking changes to consumers; these
//! types are exported to TypeScript via ts-rs.

use crate::id::UserId;
use serde::Serialize;
use ts_rs::TS;
use utoipa::ToSchema;

/// Response body for `GET /me`.
///
/// **Wire format we own (API contract with the SPA).** Exported to TypeScript
/// as `MeResponse` via ts-rs. Changes are breaking changes to the frontend.
///
/// `id` is the Hanko subject (= `users.id`); see
/// `apps/platform/src/entities/users.rs`.
#[derive(Debug, Clone, Serialize, TS, ToSchema)]
#[ts(export, export_to = "types-app/src/generated/auth/")]
pub struct MeResponse {
    pub id: UserId,
    pub email: String,
}
