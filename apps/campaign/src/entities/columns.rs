//! Bridge layer between branded ID types (defined in `campaign-shared`) and
//! sea-orm. These wrappers exist for one reason: Rust's orphan rule. We can't
//! impl sea-orm's `TryGetable` / `ValueType` / `Nullable` for branded IDs from
//! this crate (foreign trait + foreign type), and we don't want sea-orm
//! depending on `crates/campaign-shared` (the rule that shared crates stay
//! types-only). Instead, `ulid_id_column!` declares local newtype wrappers
//! around `ulid::Ulid` and hand-rolls the four sea-orm traits, serializing
//! through Crockford base32 TEXT on disk. `From` impls move values across
//! the entity/domain boundary.
//!
//! The `*Col` types live entirely inside this crate; nothing outside
//! `apps/campaign/` imports them.

use familiar_systems_campaign_shared::id::{BlockId, PageId};
use familiar_systems_campaign_shared::page_kind::PageKind;
use familiar_systems_campaign_shared::status::Status;
use sea_orm::sea_query::{ArrayType, ColumnType, Nullable, ValueType, ValueTypeErr};
use sea_orm::{
    ColIdx, DbErr, DeriveActiveEnum, EnumIter, QueryResult, TryFromU64, TryGetError, TryGetable,
    Value,
};

// ULID-backed branded IDs (BlockId, SessionId, SuggestionId, ConversationId)
// can't use `DeriveValueType` directly. The derive needs the inner type to
// already implement sea-orm's `Into<Value>` / `TryGetable` / `ValueType` /
// `Nullable`. `String` and `uuid::Uuid` do (sea-orm ships impls for both).
// `ulid::Ulid` doesn't, and we can't add them (orphan rule: foreign trait,
// foreign type). So we hand-roll the four traits per ULID column type, going
// through `String` (Crockford base32) for the on-disk representation. That's
// the encoding vec0 needs anyway: TEXT primary keys work in vec0; BLOB ones
// don't.
//
// `ulid_id_column!` reuses this scaffolding for any ULID-backed branded ID.
// When sessions/suggestions/conversations get tables we add lines, not files.
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
                ColumnType::Text
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

ulid_id_column!(PageIdCol, PageId);
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

// The on-disk representation matches `PageKind::as_loro_str` (single tokens, so
// the DB and Loro/wire strings coincide). Adding a `PageKind` variant adds a
// line here; the `From` matches below then fail to compile until updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum PageKindCol {
    #[sea_orm(string_value = "entity")]
    Entity,
    #[sea_orm(string_value = "template")]
    Template,
}

impl From<PageKind> for PageKindCol {
    fn from(k: PageKind) -> Self {
        match k {
            PageKind::Entity => Self::Entity,
            PageKind::Template => Self::Template,
        }
    }
}
impl From<PageKindCol> for PageKind {
    fn from(k: PageKindCol) -> Self {
        match k {
            PageKindCol::Entity => Self::Entity,
            PageKindCol::Template => Self::Template,
        }
    }
}
