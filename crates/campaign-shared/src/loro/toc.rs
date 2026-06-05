//! Table of Contents schema: container/key constants and ts-rs-exported entry types.
//!
//! The ToC is a collaborative, nested tree of entries backed by `LoroTree`.
//! Each entry is either a `Folder` (organizational container with no page)
//! or a `Page` (pointer to a page by `PageId`). Both can have children.
//! A Page can function as a folder (it has children and a page), but a
//! Folder is not a Page (no page, no `PageId`).
//!
//! This module holds only the cross-language schema. The Rust-side
//! `LoroTocDoc` wrapper that implements `CrdtDoc` lives in
//! `apps/campaign/src/loro/toc.rs`.
//!
//! ## CRDT lifecycle
//!
//! The ToC CRDT is a live view and hot cache. On campaign checkout it is
//! rebuilt from SQLite; during the active session it serves real-time
//! collaboration; a debounce timer writes changes back to SQLite. On
//! eviction the doc is dropped and rebuilt fresh on the next checkout.
//! Tombstones never accumulate across checkout cycles.
//!
//! ## Suggestion model (not yet implemented)
//!
//! Suggestions live in the CRDT alongside content entries. On checkout the
//! TocActor builds the structural tree first (fast first paint), then
//! injects pending suggestions from SQLite as CRDT updates that stream to
//! clients through the normal loro-protocol sync channel. SQLite remains
//! authoritative for suggestion lifecycle (accept/reject/expire).
//!
//! Each entry carries a `schema` discriminant:
//! - `"content"`: real entry (Folder or Page) with optional inline
//!   suggestions (`"change"` / `"delete"`) proposed by conversations.
//! - `"suggestion"`: a proposed new entry from a conversation, visible
//!   only to the GM. Carries `conversation_id` and proposed metadata.
//!
//! Visibility uses the `Status` enum from `status.rs` (`GmOnly` | `Known`
//! | `Retconned`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{ConversationId, PageId};
use crate::status::Status;

// ── Schema: Loro container names ────────────────────────────────────────────

/// Top-level LoroTree container for the table of contents.
pub const CONTAINER_TOC: &str = "toc";

// ── Schema: LoroMap key constants ───────────────────────────────────────────

pub const KEY_KIND: &str = "kind";
pub const KEY_TITLE: &str = "title";
pub const KEY_PAGE_ID: &str = "pageId";
pub const KEY_VISIBILITY: &str = "visibility";
pub const KEY_CONVERSATION_ID: &str = "conversationId";

/// Maximum nesting depth for ToC entries. Ex:
/// - One
/// - - Two
/// - - - Three
pub const MAX_DEPTH: usize = 3;

// ── Schema: kind string values ──────────────────────────────────────────────

pub const KIND_FOLDER: &str = "folder";
pub const KIND_PAGE: &str = "page";
pub const KIND_SUGGESTION: &str = "suggestion";

// ── Schema: domain types ────────────────────────────────────────────────────

/// Discriminant-only enum for filtering and display.
/// Derivable from `TocEntry` via `TocEntry::kind()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum TocEntryKind {
    Folder,
    Page,
    Suggestion,
}

/// A single ToC entry as a discriminated union.
///
/// Each variant carries only the fields valid for that kind, making invalid
/// states unrepresentable. A `Page` can have children (functioning as a
/// folder with a page), but a `Folder` is not a Page (organizational
/// container only, no page).
///
/// `Suggestion` entries are proposed new entries from AI conversations,
/// visible only to the GM until accepted or rejected.
///
/// **Loro storage** uses variant-specific keys (`pageId`, `conversationId`).
/// Conversion between flat LoroMap fields and this enum happens in `LoroTocDoc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum TocEntry {
    #[serde(rename = "folder")]
    Folder {
        title: String,
        visibility: Status,
        #[serde(default)]
        suggestions: Vec<TocSuggestion>,
    },
    #[serde(rename = "page")]
    Page {
        title: String,
        #[serde(rename = "pageId")]
        page_id: PageId,
        visibility: Status,
        #[serde(default)]
        suggestions: Vec<TocSuggestion>,
    },
    /// Proposed new entry from an AI conversation. GM-only until accepted.
    #[serde(rename = "suggestion")]
    Suggestion {
        #[serde(rename = "conversationId")]
        conversation_id: ConversationId,
        title: Option<String>,
        visibility: Status,
    },
}

/// An inline suggestion on an existing content entry (Folder or Page).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "schema", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum TocSuggestion {
    /// Proposed changes to an existing entry's metadata.
    #[serde(rename = "change")]
    Change {
        #[serde(rename = "conversationId")]
        conversation_id: ConversationId,
        proposed: ProposedChange,
    },
    /// Proposed deletion of an existing entry.
    #[serde(rename = "delete")]
    Delete {
        #[serde(rename = "conversationId")]
        conversation_id: ConversationId,
    },
}

/// What a `TocSuggestion::Change` proposes to modify.
/// At least one field is always present.
///
/// Note that _suggested_ changes in visibility are always only GM-viewable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "scope", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum ProposedChange {
    #[serde(rename = "both")]
    Both { title: String, visibility: Status },
    #[serde(rename = "title")]
    Title { title: String },
    #[serde(rename = "visibility")]
    Visibility { visibility: Status },
}

impl TocEntry {
    pub fn kind(&self) -> TocEntryKind {
        match self {
            Self::Folder { .. } => TocEntryKind::Folder,
            Self::Page { .. } => TocEntryKind::Page,
            Self::Suggestion { .. } => TocEntryKind::Suggestion,
        }
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            Self::Folder { title, .. } | Self::Page { title, .. } => Some(title),
            Self::Suggestion { title, .. } => title.as_deref(),
        }
    }

    pub fn visibility(&self) -> &Status {
        match self {
            Self::Folder { visibility, .. }
            | Self::Page { visibility, .. }
            | Self::Suggestion { visibility, .. } => visibility,
        }
    }
}
