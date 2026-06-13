//! Page document schema: container/key constants and ts-rs-exported types.
//!
//! The Page LoroDoc has a `"meta"` root container plus one root container per
//! section. For Entity and Template pages the layout is:
//! - `"meta"` (LoroMap): title, status, and kind as LWW strings
//! - `"preamble"` (LoroMap): the bounded "index card" section (starts empty)
//! - `"body"` (LoroMap): the freeform ProseMirror document root
//!
//! The ordered section list is a function of the page's `kind`
//! ([`PageKind::sections`](crate::page_kind::PageKind::sections)); future kinds
//! (Skill, Session) declare their own. Each ProseMirror section maps to its own
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
pub const CONTAINER_META: &str = "meta";

/// Top-level LoroMap for the bounded "index card" section of a Page.
/// Starts empty; AI-authored/maintained per the multi-section design. Must be
/// pre-initialized in `new()` to avoid the concurrent sub-container creation
/// hazard.
pub const CONTAINER_PREAMBLE: &str = "preamble";

/// Top-level LoroMap for the freeform body section of a Page.
/// Structure inside is managed by loro-prosemirror on the client and by
/// the block codec on the server. Must be pre-initialized in `new()` to
/// avoid the concurrent sub-container creation hazard.
pub const CONTAINER_BODY: &str = "body";

// ── Schema: LoroMap key constants ───────────────────────────────────────────

pub const KEY_TITLE: &str = "title";
pub const KEY_STATUS: &str = "status";
/// The `PageKind` discriminant, stored as the LWW string from
/// [`PageKind::as_loro_str`](crate::page_kind::PageKind::as_loro_str).
/// Distinct from the ToC tree's own `kind` (`TocEntryKind`): that one lives on
/// ToC tree nodes, this one on a Page's `meta` map.
pub const KEY_KIND: &str = "kind";

// ── Schema: section identifiers ─────────────────────────────────────────────
//
// A section's at-rest `blocks.section` value is identical to its Loro root
// container name. These aliases keep that equality explicit and un-driftable:
// the actor persists rows under `SECTION_*` and the doc opens the matching
// `CONTAINER_*` map, and they are the same string by construction.

/// Section name for the preamble; stored in the `section` column of `blocks`.
pub const SECTION_PREAMBLE: &str = CONTAINER_PREAMBLE;

/// Section name for the body; stored in the `section` column of `blocks`.
pub const SECTION_BODY: &str = CONTAINER_BODY;

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
