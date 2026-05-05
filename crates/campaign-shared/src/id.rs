//! Campaign-scoped ID types.
//!
//! These IDs identify entities that exist within a campaign. The platform server
//! never uses them directly.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident, uuid, $brand:expr) => {
        define_id!(@inner $(#[$meta])* $name, uuid::Uuid, uuid::Uuid::now_v7(), $brand);
    };
    ($(#[$meta:meta])* $name:ident, ulid, $brand:expr) => {
        define_id!(@inner $(#[$meta])* $name, ulid::Ulid, ulid::Ulid::new(), $brand);
    };
    ($(#[$meta:meta])* $name:ident, nanoid, $brand:expr) => {
        define_id!(@inner $(#[$meta])* $name, String, nanoid::nanoid!(), $brand);
    };
    (@inner $(#[$meta:meta])* $name:ident, $inner:ty, $ctor:expr, $brand:expr) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
        #[ts(export, export_to = "types-campaign/src/generated/id/", type = $brand)]
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
    /// Uniquely identifies a thing (NPC, location, item, etc.).
    /// Kept as a nanoid for short URLs.
    ThingId,
    nanoid,
    "string & { readonly __brand: \"ThingId\" }"
);
define_id!(
    /// Uniquely identifies a block within a document. ULID gives approximate
    /// chronological creation ordering and a single representation across
    /// regular tables (TEXT primary key) and sqlite-vec virtual tables (which
    /// only accept INTEGER or TEXT primary keys, not BLOB).
    BlockId,
    ulid,
    "string & { readonly __brand: \"BlockId\" }"
);
define_id!(
    /// Identifies a play session (discord call, table session, etc.).
    SessionId,
    ulid,
    "string & { readonly __brand: \"SessionId\" }"
);
define_id!(
    /// Uniquely identifies a journal.
    /// Kept as a nanoid for short URLs (same rationale as ThingId).
    JournalId,
    nanoid,
    "string & { readonly __brand: \"JournalId\" }"
);
define_id!(
    /// Identifies a specific suggestion made by an AI assistant.
    SuggestionId,
    ulid,
    "string & { readonly __brand: \"SuggestionId\" }"
);
define_id!(
    /// Identifies a specific conversation between one or more users and an AI assistant.
    ConversationId,
    ulid,
    "string & { readonly __brand: \"ConversationId\" }"
);
