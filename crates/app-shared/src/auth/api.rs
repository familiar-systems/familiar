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
/// `id` is the Hanko subject (= `users.id`).
#[derive(Debug, Clone, Serialize, TS, ToSchema)]
#[ts(export, export_to = "types-app/src/generated/auth/")]
pub struct MeResponse {
    pub id: UserId,
    pub email: String,
}
