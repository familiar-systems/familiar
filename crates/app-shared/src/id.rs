//! Application-level ID types.
//!
//! Only IDs that cross the platform/campaign boundary live here.
//! Campaign-scoped IDs (ThingId, BlockId, etc.) are in `campaign-shared`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, uuid, $brand:expr) => {
        define_id!(@inner $(#[$meta])* $name, uuid::Uuid, uuid::Uuid::now_v7(), $brand, "uuid");
    };
    ($(#[$meta:meta])* $name:ident, nanoid, $brand:expr) => {
        define_id!(@inner $(#[$meta])* $name, String, nanoid::nanoid!(), $brand, "nanoid");
    };
    (@inner $(#[$meta:meta])* $name:ident, $inner:ty, $ctor:expr, $brand:expr, $format:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, ToSchema)]
        #[ts(export, export_to = "types-app/src/generated/id/", type = $brand)]
        // Force utoipa to emit a named component (string + format) rather
        // than inlining the inner type. Codegen on the TS side replaces the
        // $ref with an import of the ts-rs branded type, so the SPA sees
        // the brand even though OpenAPI itself can't model it.
        #[schema(value_type = String, format = $format)]
        pub struct $name(pub $inner);

        #[allow(clippy::new_without_default)]
        impl $name {
            pub fn new() -> Self {
                Self($ctor)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

define_id!(
    /// Identifies a campaign. Kept as a nanoid for short URLs.
    CampaignId,
    nanoid,
    "string & { readonly __brand: \"CampaignId\" }"
);
define_id!(
    /// Uniquely identifies a user.
    UserId,
    uuid,
    "string & { readonly __brand: \"UserId\" }"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nanoid_ids_are_unique() {
        let a = CampaignId::new();
        let b = CampaignId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn uuid7_ids_are_unique() {
        let a = UserId::new();
        let b = UserId::new();
        assert_ne!(a, b);
    }

    #[test]
    fn serde_roundtrip_nanoid() {
        let id = CampaignId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: CampaignId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn serde_roundtrip_uuid() {
        let id = UserId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
