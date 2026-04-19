use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

/// Local projection of a Hanko user.
///
/// `hanko_sub` is the stable join key against Hanko (OIDC `sub` claim);
/// `email` is mirrored here so downstream features (campaign invites,
/// billing, notifications) can query by email without a round-trip to Hanko.
///
/// **Email is NOT NULL + UNIQUE here even though Hanko itself allows null
/// email** (passkey-only accounts are supported upstream). The product
/// invariant "every user has a verified email" is enforced at the auth
/// boundary in `crates/app-shared/src/auth/domain.rs`
/// (`HankoClaims::try_from`), so by the time a row is inserted here the
/// email has already been validated. Keeping the DB column non-null means
/// no code path downstream of this table has to reason about the
/// null-email case.
///
/// Upstream Hanko user object schema (for contrast):
/// <https://docs.hanko.io/api-reference/public/user-management/get-a-user-by-id>
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    /// Stable Hanko identifier (OIDC `sub` claim). Never changes across email or profile updates.
    #[sea_orm(unique)]
    pub hanko_sub: String,
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
        let m = Model {
            id: Uuid::now_v7(),
            hanko_sub: "sub-1".into(),
            email: "a@b.com".into(),
            created_at: now,
            updated_at: now,
        };
        let am: ActiveModel = m.clone().into();
        // Model -> ActiveModel marks all fields Unchanged (they came from an existing row).
        // Set is for fields you intend to write; Unchanged preserves the loaded value.
        assert_eq!(am.id, Unchanged(m.id));
        assert_eq!(am.hanko_sub, Unchanged("sub-1".to_string()));
        assert_eq!(am.email, Unchanged("a@b.com".to_string()));
        assert_eq!(am.created_at, Unchanged(now));
        assert_eq!(am.updated_at, Unchanged(now));
    }
}
