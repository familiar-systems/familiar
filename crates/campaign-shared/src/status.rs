use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

/// Visibility status for campaign content. The CRDT syncs all content to all
/// clients regardless of status; consumers (the browser UI, AI conversations)
/// filter what they surface based on the user's role.
/// See: docs/plans/2026-02-22-ai-prd.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, ToSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum Status {
    /// This is known only to the GM.
    /// It could be a secret plot point or hidden story arc.
    /// Or it could be some piece of lore or background that the GM hasn't decided on yet.
    /// Regardless, only the GM is aware of it but AI treats it as fact.
    GmOnly,
    // TODO(rename): docs now use `player_visible` as the canonical term for this
    // variant (docs/plans/2026-06-29-templates.md; the Status glossary entry).
    // Rename `Known` -> `PlayerVisible` and propagate the vocabulary to `StatusCol`
    // (apps/campaign/src/entities/columns.rs), the generated `Status.ts`, and call
    // sites. Decide the at-rest/wire token separately: keep `known` / `gmOnly`
    // frozen via `#[serde(rename = "known")]` (and unchanged `as_loro_str` /
    // `string_value`) for zero migration, or re-spell with a `blocks`/`pages`
    // status migration if campaign data must be preserved. Do this after the docs
    // land.
    /// This is known to players.
    /// It has either been revealed through play or the GM has explicitly shared it.
    Known,
    /// This was canon but has been retconned during play.
    Retconned,
}

impl Status {
    /// The camelCase string used to store this status inside Loro CRDT docs.
    ///
    /// This is the same representation `#[serde(rename_all = "camelCase")]`
    /// produces, which is what the client reads directly out of the doc (see the
    /// generated `types-campaign` `Status.ts`). We keep it an explicit `match`
    /// rather than deriving it from serde so the persisted CRDT format stays
    /// pinned even if the enum is later renamed - a variant rename should not
    /// silently migrate data already written to object storage. The drift test
    /// below guards that this mapping still agrees with serde, so the wire
    /// contract with the TS type can't break unnoticed.
    pub fn as_loro_str(&self) -> &'static str {
        match self {
            Status::GmOnly => "gmOnly",
            Status::Known => "known",
            Status::Retconned => "retconned",
        }
    }

    /// Parse the Loro/wire string back into a `Status`. Returns `None` for an
    /// unrecognized value so callers can treat a malformed doc field as absent.
    pub fn from_loro_str(s: &str) -> Option<Status> {
        match s {
            "gmOnly" => Some(Status::GmOnly),
            "known" => Some(Status::Known),
            "retconned" => Some(Status::Retconned),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant the enum can hold. The compiler forces the two `match`
    /// arms in the impl to stay exhaustive; this list keeps the tests covering
    /// each variant.
    const ALL: [Status; 3] = [Status::GmOnly, Status::Known, Status::Retconned];

    #[test]
    fn loro_str_round_trips() {
        for status in ALL {
            assert_eq!(Status::from_loro_str(status.as_loro_str()), Some(status));
        }
    }

    #[test]
    fn loro_str_matches_serde_representation() {
        // The string stored in the doc is a contract with the generated TS
        // `Status` type, which is derived from serde. If `rename_all` changes or
        // a variant is renamed, the serde output moves; this catches the
        // explicit `as_loro_str` map drifting away from it.
        for status in ALL {
            let serde_str = serde_json::to_value(status)
                .unwrap()
                .as_str()
                .expect("Status serializes to a JSON string")
                .to_string();
            assert_eq!(status.as_loro_str(), serde_str);
        }
    }

    #[test]
    fn from_loro_str_rejects_unknown() {
        // Guards against accidentally accepting the snake_case DB representation
        // (`gm_only`) or other stray values as the Loro/wire form.
        assert_eq!(Status::from_loro_str("gm_only"), None);
        assert_eq!(Status::from_loro_str(""), None);
    }
}
