//! Thing document schema: container/key constants and ts-rs-exported types.
//!
//! The Thing LoroDoc has two root containers:
//! - `"meta"` (LoroMap): title and status as LWW strings
//! - `"content"` (LoroMap): ProseMirror document root for the main content
//!
//! Future Thing types may add additional section containers (e.g., a Claude
//! skill might have `"trigger"` and `"core"` sections). Each ProseMirror
//! section maps to its own root-level LoroMap, scoped via `containerId`
//! on the client's `LoroSyncPlugin`.
//!
//! The Rust-side `LoroThingDoc` wrapper that implements `CrdtDoc` lives in
//! `apps/campaign/src/loro/thing.rs`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::ThingId;

// ── Schema: Loro container names ────────────────────────────────────────────

/// Top-level LoroMap for Thing metadata (title, status).
/// Values are LWW strings, not LoroText (concurrent edits pick one winner).
pub const CONTAINER_META: &str = "meta";

/// Top-level LoroMap for the main ProseMirror content section.
/// Structure inside is managed by loro-prosemirror on the client and by
/// the block codec on the server. Must be pre-initialized in `new()` to
/// avoid the concurrent sub-container creation hazard.
pub const CONTAINER_CONTENT: &str = "content";

// ── Schema: LoroMap key constants ───────────────────────────────────────────

pub const KEY_TITLE: &str = "title";
pub const KEY_STATUS: &str = "status";

// ── Schema: section identifiers ─────────────────────────────────────────────

/// Default section name for the main content body of a Thing.
/// Stored in the `section` column of the blocks table.
pub const SECTION_CONTENT: &str = "content";

/// Lightweight reference to a Thing: its ID and display name.
///
/// Used by CampaignVocabulary, ToC, RelationshipGraph, and anywhere
/// that needs to reference a Thing without loading its full document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub struct ThingHandle {
    pub id: ThingId,
    pub name: String,
}
