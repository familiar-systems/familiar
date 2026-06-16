//! Page document schema: container/key constants and ts-rs-exported types.
//!
//! The Page LoroDoc has a `"meta"` root container plus one root container per
//! section. For Entity and Template pages the layout is:
//! - `"meta"` (LoroMap): title, status, and kind as LWW strings
//! - `"preamble"` (LoroMap): the bounded "index card" section (starts empty)
//! - `"body"` (LoroMap): the freeform ProseMirror document root
//!
//! The ordered section list is a function of the page's `kind`
//! ([`PageKind::sections`](crate::page_kind::PageKind::sections)); Session
//! declares prep / summary / journal / transcript, and the future Skill kind
//! will declare its own. Each ProseMirror section maps to its own
//! root-level LoroMap, scoped via `containerId` on the client's
//! `LoroSyncPlugin`. See
//! `docs/plans/2026-06-07-multi-section-document-structure.md`.
//!
//! The Rust-side `LoroPageDoc` wrapper that implements `CrdtDoc` lives in
//! `apps/campaign/src/loro/page.rs`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::PageId;

// ── Schema: Loro container names ────────────────────────────────────────────

/// Top-level LoroMap for Page metadata (title, status).
/// Values are LWW strings, not LoroText (concurrent edits pick one winner).
/// `meta` is deliberately not a [`Section`]: it is metadata, not in
/// [`PageKind::sections`](crate::page_kind::PageKind::sections).
pub const CONTAINER_META: &str = "meta";

// ── Schema: LoroMap key constants ───────────────────────────────────────────

pub const KEY_TITLE: &str = "title";
pub const KEY_STATUS: &str = "status";
/// The `PageKind` discriminant, stored as the LWW string from
/// [`PageKind::as_loro_str`](crate::page_kind::PageKind::as_loro_str).
/// Distinct from the ToC tree's own `kind` (`TocEntryKind`): that one lives on
/// ToC tree nodes, this one on a Page's `meta` map.
pub const KEY_KIND: &str = "kind";

// ── Schema: sections ────────────────────────────────────────────────────────

/// A Page section: a named Loro root container. [`as_str`](Section::as_str) is
/// the container id and the client's `containerId` (the wire string, mirrored
/// in `@familiar-systems/editor` and pinned by its `section-contract` test).
/// Every declared section is pre-initialized at doc construction (see
/// `LoroPageDoc::from_blocks`) to avoid Loro's concurrent sub-container-creation
/// hazard.
///
/// This is the wire/domain view. The at-rest DB token is a separate, frozen
/// enum (`SectionCol`, app-local to `apps/campaign`): persisting through it
/// decouples the `blocks.section` column from `as_str`, so a section can be
/// re-spelled, localized, or have its grammar fixed without a DB migration.
/// Same split as [`PageKind`](crate::page_kind::PageKind) / `PageKindCol`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    /// The bounded "index card" section (AI-authored/maintained; no headings).
    /// Entity / Template.
    Preamble,
    /// The permissive freeform ProseMirror section. Entity / Template.
    Body,
    // Session sections (kind == Session). See
    // docs/plans/2026-06-07-multi-section-document-structure.md.
    /// GM prep written before play: plans, contingencies, @mentions.
    Prep,
    /// The post-play recap (the audio GM summary or the no-audio recap).
    Summary,
    /// The canonical narrative journal, the session's durable output.
    Journal,
    /// The audio transcript, held in-doc for now (a future split into its own
    /// room/store is lossless; the durable truth is the `blocks` rows).
    Transcript,
    // Future kinds declare their own sections too: Skill = `Description` + `Body`.
}

impl Section {
    /// The Loro root-container id and at-rest section string. An explicit
    /// `match` (the single source of truth) so a rename can't silently migrate
    /// the wire format; mirrors
    /// [`PageKind::as_loro_str`](crate::page_kind::PageKind::as_loro_str).
    pub fn as_str(&self) -> &'static str {
        match self {
            Section::Preamble => "preamble",
            Section::Body => "body",
            Section::Prep => "prep",
            Section::Summary => "summary",
            Section::Journal => "journal",
            Section::Transcript => "transcript",
        }
    }
}

/// Lightweight reference to a Page: its ID and display name.
///
/// Used by CampaignVocabulary, ToC, RelationshipGraph, and anywhere
/// that needs to reference a Page without loading its full document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct PageHandle {
    pub id: PageId,
    pub name: String,
}
