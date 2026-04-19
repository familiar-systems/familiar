use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;
use serde::Serialize;
use uuid::Uuid;

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
