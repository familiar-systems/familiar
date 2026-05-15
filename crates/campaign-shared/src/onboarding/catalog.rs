//! `GET /catalog/systems` response shapes.
//!
//! Locale-resolved on the campaign side; the FE consumes plain strings.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct CatalogResponse {
    pub systems: Vec<SystemEntry>,
    /// The "bring your own" affordance: a single always-visible card
    /// rendered below the catalog list. Not a system itself; the wizard
    /// resolves its display copy and default template bundle from this.
    pub byo: ByoEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct SystemEntry {
    /// Slug, kebab-case. Stable across locales.
    pub id: String,
    pub name: String,
    pub tagline: String,
    /// Hex color (`#rrggbb`) used by the wizard's system picker.
    pub color: String,
    pub popular: bool,
    pub bundle: Vec<TemplateRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct ByoEntry {
    /// Default template bundle when BYO is selected. The BYO card's
    /// display copy (title, body, empty-input fallback, swatch color)
    /// lives in the wizard frontend alongside the rest of the wizard's
    /// UI strings; only the bundle is catalog-maintainer configuration,
    /// so only the bundle ships through this struct.
    pub bundle: Vec<TemplateRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "types-campaign/src/generated/onboarding/")]
pub struct TemplateRef {
    /// `<system-id>/<template-name>` slug, e.g. `common/npc`.
    pub slug: String,
    pub name: String,
    pub description: String,
    /// `lucide-react` icon name.
    pub icon: String,
}
