//! Thing document schema: ts-rs-exported handle type.
//!
//! The Rust-side `LoroThingDoc` wrapper that implements `CrdtDoc` lives in
//! `apps/campaign/src/loro/thing.rs`. Only the cross-language type belongs
//! here.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::id::ThingId;

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
