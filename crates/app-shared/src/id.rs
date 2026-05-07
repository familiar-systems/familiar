//! Application-level ID types.
//!
//! Only IDs that cross the platform/campaign boundary live here.
//! Campaign-scoped IDs (ThingId, BlockId, etc.) are in `campaign-shared`.

use fs_id::{Nanoid, Uuid, fs_id};

/// Identifies a campaign. Kept as a nanoid for short URLs.
#[fs_id(export_to = "types-app/src/generated/id/")]
pub struct CampaignId(pub Nanoid);

/// Uniquely identifies a user.
#[fs_id(export_to = "types-app/src/generated/id/")]
pub struct UserId(pub Uuid);

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
