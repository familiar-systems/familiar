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

use familiar_systems_campaign_shared::id::{BlockId, PageId, SessionId};
use familiar_systems_campaign_shared::loro::page::Section;
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
ulid_id_column!(SessionIdCol, SessionId);

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
    #[sea_orm(string_value = "session")]
    Session,
}

impl From<PageKind> for PageKindCol {
    fn from(k: PageKind) -> Self {
        match k {
            PageKind::Entity => Self::Entity,
            PageKind::Template => Self::Template,
            PageKind::Session => Self::Session,
        }
    }
}
impl From<PageKindCol> for PageKind {
    fn from(k: PageKindCol) -> Self {
        match k {
            PageKindCol::Entity => Self::Entity,
            PageKindCol::Template => Self::Template,
            PageKindCol::Session => Self::Session,
        }
    }
}

// The frozen on-disk token for a Page section. Decoupled from the wire/Loro id
// (`Section::as_str`): persisting through this boundary means a section can be
// re-spelled / localized later without a DB migration, because the `From` impls
// map by *variant*, not by string. The tokens coincide with `as_str` today (a
// drift test guards that), the same convenience `PageKindCol` notes. The
// boundary is strict: an unknown at-rest token fails the read (sea-orm rejects
// an unrecognized `string_value`) rather than dropping silently -- the same
// posture as `PageKindCol` / `StatusCol`. Adding a `Section` variant adds a line
// here; the `From` matches below then fail to compile until updated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, DeriveActiveEnum)]
#[sea_orm(rs_type = "String", db_type = "Text")]
pub enum SectionCol {
    #[sea_orm(string_value = "preamble")]
    Preamble,
    #[sea_orm(string_value = "body")]
    Body,
    #[sea_orm(string_value = "prep")]
    Prep,
    #[sea_orm(string_value = "summary")]
    Summary,
    #[sea_orm(string_value = "transcript")]
    Transcript,
    #[sea_orm(string_value = "journal")]
    Journal,
}

impl From<Section> for SectionCol {
    fn from(s: Section) -> Self {
        match s {
            Section::Preamble => Self::Preamble,
            Section::Body => Self::Body,
            Section::Prep => Self::Prep,
            Section::Summary => Self::Summary,
            Section::Transcript => Self::Transcript,
            Section::Journal => Self::Journal,
        }
    }
}
impl From<SectionCol> for Section {
    fn from(s: SectionCol) -> Self {
        match s {
            SectionCol::Preamble => Self::Preamble,
            SectionCol::Body => Self::Body,
            SectionCol::Prep => Self::Prep,
            SectionCol::Summary => Self::Summary,
            SectionCol::Transcript => Self::Transcript,
            SectionCol::Journal => Self::Journal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::ActiveEnum;

    #[test]
    fn section_col_round_trips_known_tokens() {
        for col in [
            SectionCol::Preamble,
            SectionCol::Body,
            SectionCol::Prep,
            SectionCol::Summary,
            SectionCol::Transcript,
            SectionCol::Journal,
        ] {
            assert_eq!(SectionCol::try_from_value(&col.to_value()).unwrap(), col);
        }
    }

    #[test]
    fn section_col_rejects_unknown_token() {
        // Strict boundary: an at-rest token this binary doesn't know (legacy /
        // rename debris, or a newer shard's section seen during rollback) fails
        // the read rather than silently dropping -- the posture `PageKindCol` /
        // `StatusCol` already take. This is where the unknown-token concern lives
        // now that `from_blocks` buckets on typed `Section`s.
        assert!(SectionCol::try_from_value(&"content".to_string()).is_err());
    }

    #[test]
    fn section_col_db_token_matches_wire_string_today() {
        // The DB token and the Loro/wire id coincide now but are decoupled by
        // design; guard that they still agree, so a divergence has to be a
        // deliberate edit (and a migration) rather than an accident.
        for section in [
            Section::Preamble,
            Section::Body,
            Section::Prep,
            Section::Summary,
            Section::Transcript,
            Section::Journal,
        ] {
            assert_eq!(SectionCol::from(section).to_value(), section.as_str());
        }
    }
}
