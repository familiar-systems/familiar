//! Branded ID infrastructure shared by familiar-systems crates.
//!
//! Defines the inner types ([`Nanoid`] newtype + re-exported [`Uuid`] /
//! [`Ulid`]), the [`BrandedInner`] / [`BrandedInnerAuto`] traits they
//! implement, and re-exports the [`fs_id`] attribute macro from
//! `fs-id-macros`.
//!
//! ```ignore
//! use fs_id::{fs_id, Nanoid};
//!
//! #[fs_id(export_to = "types-app/src/generated/id/")]
//! pub struct CampaignId(pub Nanoid);
//! ```

pub use fs_id_macros::fs_id;
pub use ulid::Ulid;
pub use uuid::Uuid;

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

/// Nanoid-backed string ID.
///
/// Newtype around `String` so it can implement [`BrandedInner`] without
/// blanket-impling for every `String` (which would conflict with `u64`
/// branding for [`crate::Uuid`] etc.).
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

/// JSON primitive a branded ID type wraps.
///
/// The proc macro emits `<Inner as BrandedInner>::TS_PRIMITIVE` indirectly
/// via its allow-list match (ts-rs needs the brand string at expansion
/// time, before trait consts are available). The trait itself is the
/// single source of truth for the runtime/serde shape.
pub trait BrandedInner: Sized {
    /// `"string"` or `"number"`.
    const TS_PRIMITIVE: &'static str;
}

/// Inner types that mint a fresh value with no input.
///
/// Numeric IDs (server-assigned counters, etc.) deliberately don't impl
/// this — their `new()` takes a value.
pub trait BrandedInnerAuto: BrandedInner {
    fn generate() -> Self;
}

impl BrandedInner for Nanoid {
    const TS_PRIMITIVE: &'static str = "string";
}
impl BrandedInner for Uuid {
    const TS_PRIMITIVE: &'static str = "string";
}
impl BrandedInner for Ulid {
    const TS_PRIMITIVE: &'static str = "string";
}
impl BrandedInner for u64 {
    const TS_PRIMITIVE: &'static str = "number";
}
impl BrandedInner for u32 {
    const TS_PRIMITIVE: &'static str = "number";
}
impl BrandedInner for i64 {
    const TS_PRIMITIVE: &'static str = "number";
}
impl BrandedInner for i32 {
    const TS_PRIMITIVE: &'static str = "number";
}

impl BrandedInnerAuto for Nanoid {
    fn generate() -> Self {
        Self::new()
    }
}
impl BrandedInnerAuto for Uuid {
    fn generate() -> Self {
        Uuid::now_v7()
    }
}
impl BrandedInnerAuto for Ulid {
    fn generate() -> Self {
        Ulid::new()
    }
}
