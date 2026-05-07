//! Branded ID infrastructure shared by familiar-systems crates.
//!
//! Defines the inner types ([`Nanoid`] newtype + re-exported [`Uuid`] /
//! [`Ulid`]) and re-exports the [`fs_id`] attribute macro from
//! `fs-id-macros`. Recognized inner types are gated by the macro's
//! `classify` allow-list, not a trait — implementing a trait is not
//! sufficient to make a new inner type work.
//!
//! ```ignore
//! use fs_id::{fs_id, Nanoid};
//!
//! #[fs_id(export_to = "types-app/src/generated/id/")]
//! pub struct CampaignId(pub Nanoid);
//!
//! let fresh = CampaignId::generate();
//! let from_db = CampaignId::new(some_nanoid);
//! ```

pub use fs_id_macros::fs_id;
pub use ulid::Ulid;
pub use uuid::Uuid;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

/// Re-exports of the crates the `#[fs_id]` macro routes through.
///
/// `serde` is routed via `#[serde(crate = "...")]`; `ulid` / `uuid` via
/// the constructor expression in `Constructor::Auto`. `ts-rs` and `utoipa`
/// are intentionally not here — their derives generate code that
/// hard-codes `ts_rs::*` / `utoipa::*` paths and they have no
/// crate-rename attribute, so consumers of `#[fs_id]` need those crates
/// as direct deps regardless. Putting them here would mislead.
#[doc(hidden)]
pub mod __private {
    pub use serde;
    pub use ulid;
    pub use uuid;
}

/// Nanoid-backed string ID.
///
/// Newtype around `String` so the `#[fs_id]` allow-list can distinguish
/// nanoid-shaped inputs from arbitrary strings.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS, ToSchema)]
#[ts(type = "string")]
#[schema(value_type = String)]
pub struct Nanoid(pub String);

impl Nanoid {
    pub fn new() -> Self {
        Self(nanoid::nanoid!())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for Nanoid {
    fn default() -> Self {
        Self::new()
    }
}

impl From<String> for Nanoid {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<Nanoid> for String {
    fn from(value: Nanoid) -> Self {
        value.0
    }
}

impl AsRef<str> for Nanoid {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Nanoid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
