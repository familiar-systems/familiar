//! Bridge layer between branded ID types (defined in `campaign-shared`) and
//! sea-orm. These wrappers exist for one reason: Rust's orphan rule. We can't
//! impl sea-orm's `TryGetable` / `ValueType` / `Nullable` for `ThingId` from
//! this crate (foreign trait + foreign type), and we don't want sea-orm
//! depending on `crates/campaign-shared` (the rule that shared crates stay
//! types-only). Instead, we declare local newtype wrappers around the
//! primitive each branded ID stores; `DeriveValueType` emits the four traits
//! locally; `From` impls move values across the entity ↔ domain boundary.
//!
//! The `*Col` types live entirely inside this crate; nothing outside
//! `apps/campaign/` imports them.

use familiar_systems_campaign_shared::id::{BlockId, ThingId};
use familiar_systems_campaign_shared::status::Status;
use sea_orm::sea_query::{ArrayType, ColumnType, Nullable, ValueType, ValueTypeErr};
use sea_orm::{
    ColIdx, DbErr, DeriveActiveEnum, DeriveValueType, EnumIter, QueryResult, TryFromU64,
    TryGetError, TryGetable, Value,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash, DeriveValueType)]
pub struct ThingIdCol(pub String);

impl From<ThingId> for ThingIdCol {
    fn from(v: ThingId) -> Self {
        Self(v.0)
    }
}
impl From<ThingIdCol> for ThingId {
    fn from(v: ThingIdCol) -> Self {
        ThingId(v.0)
    }
}

// Sea-orm requires every primary-key type to implement `TryFromU64`, even when
// the key isn't numeric. The standard pattern for non-numeric PKs is to fail
// the conversion: it tells sea-orm "no, you can't construct one of these from
// an autoincrement counter," which matches reality (nanoids and UUIDs aren't
// generated from u64).
impl TryFromU64 for ThingIdCol {
    fn try_from_u64(_n: u64) -> Result<Self, DbErr> {
        Err(DbErr::ConvertFromU64("ThingIdCol"))
    }
}

// ULID-backed branded IDs (BlockId, SessionId, SuggestionId, ConversationId)
// can't use `DeriveValueType` directly. The derive needs the inner type to
// already implement sea-orm's `Into<Value>` / `TryGetable` / `ValueType` /
// `Nullable`. `String` and `uuid::Uuid` do (sea-orm ships impls for both).
// `ulid::Ulid` doesn't, and we can't add them — orphan rule (foreign trait,
// foreign type). So we hand-roll the four traits per ULID column type, going
// through `String` (Crockford base32) for the on-disk representation. That's
// the encoding vec0 needs anyway: TEXT primary keys work in vec0; BLOB ones
// don't.
//
// `ulid_id_column!` reuses this scaffolding for any ULID-backed branded ID.
// Today only blocks have an entity so we only declare `BlockIdCol`; when
// sessions/suggestions/conversations get tables we add lines, not files.
macro_rules! ulid_id_column {
    ($col:ident, $shared:path) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $col(pub ulid::Ulid);

        impl From<$shared> for $col {
            fn from(v: $shared) -> Self {
                Self(v.0)
            }
        }
        impl From<$col> for $shared {
            fn from(v: $col) -> Self {
                $shared(v.0)
            }
        }

        impl From<$col> for Value {
            fn from(v: $col) -> Self {
                Value::String(Some(Box::new(v.0.to_string())))
            }
        }

        impl TryGetable for $col {
            fn try_get_by<I: ColIdx>(res: &QueryResult, idx: I) -> Result<Self, TryGetError> {
                let s = String::try_get_by(res, idx)?;
                ulid::Ulid::from_string(&s).map(Self).map_err(|e| {
                    TryGetError::DbErr(DbErr::Custom(format!(
                        "invalid ULID in {}: {e}",
                        stringify!($col)
                    )))
                })
            }
        }

        impl ValueType for $col {
            fn try_from(v: Value) -> Result<Self, ValueTypeErr> {
                let s = <String as ValueType>::try_from(v)?;
                ulid::Ulid::from_string(&s)
                    .map(Self)
                    .map_err(|_| ValueTypeErr)
            }
            fn type_name() -> String {
                stringify!($col).to_owned()
            }
            fn array_type() -> ArrayType {
                <String as ValueType>::array_type()
            }
            fn column_type() -> ColumnType {
                <String as ValueType>::column_type()
            }
        }

        impl Nullable for $col {
            fn null() -> Value {
                <String as Nullable>::null()
            }
        }

        impl TryFromU64 for $col {
            fn try_from_u64(_n: u64) -> Result<Self, DbErr> {
                Err(DbErr::ConvertFromU64(stringify!($col)))
            }
        }
    };
}

ulid_id_column!(BlockIdCol, BlockId);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum StatusCol {
    #[sea_orm(string_value = "gm_only")]
    GmOnly,
    #[sea_orm(string_value = "known")]
    Known,
    #[sea_orm(string_value = "retconned")]
    Retconned,
}

impl From<Status> for StatusCol {
    fn from(s: Status) -> Self {
        match s {
            Status::GmOnly => Self::GmOnly,
            Status::Known => Self::Known,
            Status::Retconned => Self::Retconned,
        }
    }
}
impl From<StatusCol> for Status {
    fn from(s: StatusCol) -> Self {
        match s {
            StatusCol::GmOnly => Self::GmOnly,
            StatusCol::Known => Self::Known,
            StatusCol::Retconned => Self::Retconned,
        }
    }
}
