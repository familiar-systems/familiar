use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Local projection of a Hanko user.
///
/// Hanko is authoritative for user identity. `id` is the Hanko subject
/// (OIDC `sub`), parsed as a UUID at the auth boundary in
/// `crates/app-shared/src/auth/domain.rs` and used directly as the primary
/// key here.
///
/// **Email is NOT NULL + UNIQUE here even though Hanko itself allows null
/// email** (passkey-only accounts are supported upstream). The product
/// invariant "every user has a verified email" is enforced at the auth
/// boundary in `HankoClaims::try_from`, so by the time a row is inserted
/// here the email has been validated. The UNIQUE constraint exists for
/// billing (one customer per email); login-time collisions arise from
/// stale local state vs. Hanko's current state and surface as
/// `AppError::EmailConflict` → HTTP 409.
///
/// Upstream Hanko user object schema:
/// <https://docs.hanko.io/api-reference/public/user-management/get-a-user-by-id>
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub email: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveValue::Unchanged;

    #[test]
    fn model_into_active_model_roundtrip() {
        let now = Utc::now();
        let id = Uuid::now_v7();
        let m = Model {
            id,
            email: "a@b.com".into(),
            created_at: now,
            updated_at: now,
        };
        let am: ActiveModel = m.clone().into();
        // Model -> ActiveModel marks all fields Unchanged (they came from an existing row).
        // Set is for fields you intend to write; Unchanged preserves the loaded value.
        assert_eq!(am.id, Unchanged(id));
        assert_eq!(am.email, Unchanged("a@b.com".to_string()));
        assert_eq!(am.created_at, Unchanged(now));
        assert_eq!(am.updated_at, Unchanged(now));
    }
}
