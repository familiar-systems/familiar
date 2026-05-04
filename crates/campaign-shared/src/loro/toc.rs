//! Table of Contents schema: container/key constants and ts-rs-exported entry types.
//!
//! The ToC is a collaborative, nested list of entries backed by `LoroTree`.
//! Each entry is either plain text (a heading/divider), a link to a Thing,
//! or a link to a Journal. Entries can be nested up to a configurable max
//! depth.
//!
//! This module holds only the cross-language schema. The Rust-side
//! `LoroTocDoc` wrapper that implements `CrdtDoc` lives in
//! `apps/campaign/src/loro/toc.rs`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::{JournalId, ThingId};

// ── Schema: Loro container names ────────────────────────────────────────────

/// Top-level LoroTree container for the table of contents.
pub const CONTAINER_TOC: &str = "toc";

/// Top-level LoroMap container for ToC metadata.
pub const CONTAINER_META: &str = "meta";

// ── Schema: LoroMap key constants ───────────────────────────────────────────

pub const KEY_KIND: &str = "kind";
pub const KEY_TITLE: &str = "title";
pub const KEY_THING_ID: &str = "thingId";
pub const KEY_JOURNAL_ID: &str = "journalId";
pub const KEY_LANDING_PAGE_ID: &str = "landingPageId";

/// Maximum nesting depth for ToC entries. Ex:
/// - One
/// - - Two
/// - - - Three
pub const MAX_DEPTH: usize = 3;

// ── Schema: kind string values ──────────────────────────────────────────────

pub const KIND_TEXT: &str = "text";
pub const KIND_THING: &str = "thing";
pub const KIND_JOURNAL: &str = "journal";

// ── Schema: domain types ────────────────────────────────────────────────────

/// Discriminant-only enum for filtering and display.
/// Derivable from `TocEntry` via `TocEntry::kind()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum TocEntryKind {
    Text,
    Thing,
    Journal,
}

/// A single ToC entry as a discriminated union.
///
/// Each variant carries only the fields valid for that kind, making invalid states
/// unrepresentable (e.g., a `Text` entry cannot have a `thing_id`).
///
/// **Loro storage** uses variant-specific keys (`thingId`, `journalId`).
/// Conversion between flat LoroMap fields and this enum happens in `LoroTocDoc`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "types-campaign/src/generated/document/")]
pub enum TocEntry {
    #[serde(rename = "text")]
    Text { title: String },
    #[serde(rename = "thing")]
    Thing {
        title: String,
        #[serde(rename = "thingId")]
        thing_id: ThingId,
    },
    #[serde(rename = "journal")]
    Journal {
        title: String,
        #[serde(rename = "journalId")]
        journal_id: JournalId,
    },
}

impl TocEntry {
    pub fn kind(&self) -> TocEntryKind {
        match self {
            Self::Text { .. } => TocEntryKind::Text,
            Self::Thing { .. } => TocEntryKind::Thing,
            Self::Journal { .. } => TocEntryKind::Journal,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            Self::Text { title } | Self::Thing { title, .. } | Self::Journal { title, .. } => title,
        }
    }
}
