use serde::{Deserialize, Serialize};
use ts_rs::TS;
use utoipa::ToSchema;

use crate::loro::page::Section;

/// What kind of Page this is: its document *structure* and the systemic actions
/// the engine takes for it. A `kind` exists only when pages differ structurally
/// (a different Loro schema) or need a systemic action the engine can't infer
/// from content; editorial differences (NPC vs Location) live in tags,
/// relationships, and template lineage, not here.
/// See
/// - docs/plans/2026-03-25-ai-serialization-format-v2.md
/// - docs/glossary.md
/// - issue #155.
///
/// Only `Entity` and `Template` exist today. `Session`, `Skill`, and `Memory`
/// are known future cases (the audio pipeline and the agent system) and get
/// added as variants when those documents are actually built - each addition
/// makes the `match` arms below non-exhaustive, so the compiler points at
/// every site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS, ToSchema)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum PageKind {
    /// Authored world content such as NPCs, places, factions, items, etc.
    /// The default kind for everything a GM writes about the world.
    Entity,
    /// A template that `Entity` pages clone. Excluded from `kind == entity`
    /// listings. Has no creation path yet (template instantiation is unbuilt);
    /// the variant exists so the schema and exclusion semantics are in place.
    Template,
    // === FUTURE ===
    // Future pages we know we need but haven't built yet go here.

    // A session is the campaign's central unit and spine.
    // Has the following sections:
    // - Prep notes
    // - (Audio only) GM summary
    // - (Audio only) Audio upload
    // - (Audio only) Audio transcription
    // - (No audio only) GM/player recap
    // - Session Journal
    //
    // Content spine:
    // TTRPGs are, fundamentally, a collaborative endeavor about what happened at the table.
    // A session is a record of the events at the table.
    // All of these other pages and tools help to record state for use by the GMs and players.
    //
    // Temporal spine:
    // Sessions happen sequentially.
    // Each session is either the start of a new story arc or follows a prior one.
    // If doing a west-marches style campaign or a living world, things get a bit murkier.
    // However, even still, coarsely, this still approximately holds.
    // Session,

    // GM-authored, campaign-specific instruction available for agents to load.
    // Has a title, a trigger, and a content block.
    //
    // Page Visibility is set like any other page.
    // For example, a GM's agent cares about how to create NPCs.
    // A player's does not, saving valuable context window space.
    // Skill,

    // AI-authored, GM-curated notes the agent keeps about this campaign.
    // The agent's durable, long-term memory: patterns it has learned and
    // standing facts about how this table plays, accumulated across sessions
    // and carried forward. Same title/trigger/content shape as a Skill; what
    // differs is provenance - the AI writes memories, the GM writes skills.
    //
    // Page Visibility is set like any other page.
    // A memory formed from gm-only context stays gm-only, so a player's agent
    // never loads it.
    // Memory,
}

impl PageKind {
    /// The camelCase string used to store this kind inside Loro CRDT docs.
    ///
    /// Kept an explicit `match` rather than derived from serde so the persisted
    /// CRDT format stays pinned even if the enum is later renamed - a variant
    /// rename should not silently migrate data already written to object
    /// storage. The drift test below guards that this mapping still agrees with
    /// serde, so the wire contract with the generated TS type can't break
    /// unnoticed. Mirrors `Status::as_loro_str`.
    pub fn as_loro_str(&self) -> &'static str {
        match self {
            PageKind::Entity => "entity",
            PageKind::Template => "template",
        }
    }

    /// Parse the Loro/wire string back into a `PageKind`. Returns `None` for an
    /// unrecognized value so callers can treat a malformed doc field as absent.
    pub fn from_loro_str(s: &str) -> Option<PageKind> {
        match s {
            "entity" => Some(PageKind::Entity),
            "template" => Some(PageKind::Template),
            _ => None,
        }
    }

    /// The ordered list of [`Section`]s this kind's document is laid out from.
    /// Each names a Loro root container; its at-rest `blocks.section` token is
    /// written through `SectionCol` at the DB edge. The order is the
    /// render/restore order; [`Section::Body`] is the freeform section and stays
    /// last so genesis seeds and "content moves into body" stay well-defined.
    ///
    /// Modeled as a `match` so adding a kind (Skill, Session) is a new arm the
    /// compiler forces every section-aware site to handle. See
    /// `docs/plans/2026-06-07-multi-section-document-structure.md`.
    pub fn sections(&self) -> &'static [Section] {
        match self {
            PageKind::Entity | PageKind::Template => &[Section::Preamble, Section::Body],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant the enum can hold. The compiler forces the two `match`
    /// arms in the impl to stay exhaustive; this list keeps the tests covering
    /// each variant (and constructs every variant, so none reads as dead code
    /// even while no creation path produces `Template` yet).
    const ALL: [PageKind; 2] = [PageKind::Entity, PageKind::Template];

    #[test]
    fn loro_str_round_trips() {
        for kind in ALL {
            assert_eq!(PageKind::from_loro_str(kind.as_loro_str()), Some(kind));
        }
    }

    #[test]
    fn loro_str_matches_serde_representation() {
        // The string stored in the doc is a contract with the generated TS
        // `PageKind` type, which is derived from serde. If `rename_all` changes
        // or a variant is renamed, the serde output moves; this catches the
        // explicit `as_loro_str` map drifting away from it.
        for kind in ALL {
            let serde_str = serde_json::to_value(kind)
                .unwrap()
                .as_str()
                .expect("PageKind serializes to a JSON string")
                .to_string();
            assert_eq!(kind.as_loro_str(), serde_str);
        }
    }

    #[test]
    fn from_loro_str_rejects_unknown() {
        assert_eq!(PageKind::from_loro_str("Entity"), None);
        assert_eq!(PageKind::from_loro_str(""), None);
    }

    #[test]
    fn entity_and_template_share_preamble_body_layout() {
        // The "Now" slice: both kinds are preamble + body. `body` must stay last
        // so genesis seeds and the content->body rename keep their meaning.
        for kind in [PageKind::Entity, PageKind::Template] {
            assert_eq!(kind.sections(), &[Section::Preamble, Section::Body]);
            assert_eq!(kind.sections().last(), Some(&Section::Body));
        }
        // Pin the wire strings: these are the Loro container ids / TS containerIds
        // the editor hand-mirrors, so they must not drift.
        assert_eq!(Section::Preamble.as_str(), "preamble");
        assert_eq!(Section::Body.as_str(), "body");
    }
}
