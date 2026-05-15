//! Catalog parsing: `content/systems.yaml` and the templates each system bundles.
//!
//! The `content/` directory is embedded at build time via [`include_dir`]:
//! a missing template referenced by `systems.yaml` fails the test that
//! constructs the catalog (see [`Catalog::load_from_embedded`]); a
//! malformed YAML fails likewise. There is no runtime path that returns
//! "template not found" — by the time `axum::serve` runs, the catalog is
//! either fully resolved or the binary aborted.

use crate::starter_content::{localized::LocalizedString, template::Template};
use familiar_systems_campaign_shared::onboarding::catalog::{ByoEntry, SystemEntry, TemplateRef};
use include_dir::{Dir, include_dir};
use serde::Deserialize;
use std::collections::BTreeMap;

/// Compile-time embed of `content/`. The path is repo-root-relative because
/// `cargo build` runs at the workspace root.
static CONTENT_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../content");

#[derive(Debug, Clone)]
pub struct Catalog {
    pub systems: Vec<SystemEntry>,
    pub byo: ByoEntry,
}

// Wire shapes (`SystemEntry`, `ByoEntry`, `TemplateRef`) are defined once in
// `crates/campaign-shared/src/onboarding/catalog.rs` so ts-rs exports them
// to `packages/types-campaign` once and the FE consumes the same shape.

/// Per-system YAML row, locale-unresolved.
#[derive(Debug, Clone, Deserialize)]
struct SystemRowYaml {
    id: String,
    name: LocalizedString,
    tagline: LocalizedString,
    color: String,
    #[serde(default)]
    popular: bool,
    #[serde(default)]
    bundle: Vec<String>,
}

/// The BYO ("bring your own") affordance, locale-unresolved. Maintainer
/// configuration is just the default template bundle; the BYO card's UI
/// copy (title, body, empty-input fallback, swatch color) lives in the
/// wizard frontend alongside the rest of its hardcoded strings.
#[derive(Debug, Clone, Deserialize)]
struct ByoRowYaml {
    #[serde(default)]
    bundle: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SystemsYaml {
    systems: Vec<SystemRowYaml>,
    byo: ByoRowYaml,
}

impl Catalog {
    /// Loads the embedded catalog. Returns an error string at startup time
    /// (and from tests) if `systems.yaml` references a slug that doesn't
    /// resolve to a `content/templates/<slug>.yaml` file.
    pub fn load_from_embedded() -> Result<RawCatalog, String> {
        let systems_yaml = CONTENT_DIR
            .get_file("systems.yaml")
            .ok_or("content/systems.yaml not found in embedded directory")?
            .contents_utf8()
            .ok_or("content/systems.yaml is not valid UTF-8")?;
        let parsed: SystemsYaml =
            serde_yaml::from_str(systems_yaml).map_err(|e| format!("content/systems.yaml: {e}"))?;

        let mut templates: BTreeMap<String, Template> = BTreeMap::new();

        // Bundle slugs from real systems + the BYO bundle all share the
        // same `templates/` directory; one resolver walks both.
        let system_slug_sources = parsed.systems.iter().map(|s| (s.id.as_str(), &s.bundle));
        let byo_slug_source = std::iter::once(("byo", &parsed.byo.bundle));
        for (origin, bundle) in system_slug_sources.chain(byo_slug_source) {
            for slug in bundle {
                if templates.contains_key(slug) {
                    continue;
                }
                let path = format!("templates/{slug}.yaml");
                let file = CONTENT_DIR.get_file(&path).ok_or_else(|| {
                    format!("content/{path} not found (referenced by {origin} bundle)")
                })?;
                let raw = file
                    .contents_utf8()
                    .ok_or_else(|| format!("content/{path} is not valid UTF-8"))?;
                let template: Template =
                    serde_yaml::from_str(raw).map_err(|e| format!("content/{path}: {e}"))?;
                templates.insert(slug.clone(), template);
            }
        }

        Ok(RawCatalog {
            systems: parsed.systems,
            byo: parsed.byo,
            templates,
        })
    }

    /// Resolves a [`RawCatalog`] for `locale`, falling back to `en` per
    /// [`LocalizedString::resolve`].
    pub fn from_raw(raw: &RawCatalog, locale: &str) -> Self {
        let systems = raw
            .systems
            .iter()
            .map(|s| SystemEntry {
                id: s.id.clone(),
                name: s.name.resolve(locale).to_string(),
                tagline: s.tagline.resolve(locale).to_string(),
                color: s.color.clone(),
                popular: s.popular,
                bundle: resolve_bundle(&s.bundle, &raw.templates, locale),
            })
            .collect();
        let byo = ByoEntry {
            bundle: resolve_bundle(&raw.byo.bundle, &raw.templates, locale),
        };
        Self { systems, byo }
    }
}

fn resolve_bundle(
    slugs: &[String],
    templates: &BTreeMap<String, Template>,
    locale: &str,
) -> Vec<TemplateRef> {
    slugs
        .iter()
        .map(|slug| {
            let t = templates.get(slug).expect("validated at load time");
            TemplateRef {
                slug: slug.clone(),
                name: t.meta.name.resolve(locale).to_string(),
                description: t.meta.description.resolve(locale).to_string(),
                icon: t.meta.icon.clone(),
            }
        })
        .collect()
}

/// Locale-unresolved catalog. Held in `AppState` and resolved per-request.
#[derive(Debug, Clone)]
pub struct RawCatalog {
    systems: Vec<SystemRowYaml>,
    byo: ByoRowYaml,
    templates: BTreeMap<String, Template>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_catalog_loads_without_error() {
        let raw = Catalog::load_from_embedded().expect("embedded catalog should parse");
        // Lock the shape we expect from content/systems.yaml today. If
        // the YAML adds/removes a system, this assertion is the right
        // place to update.
        let ids: Vec<&str> = raw.systems.iter().map(|s| s.id.as_str()).collect();
        assert!(
            ids.contains(&"dnd-5e"),
            "expected dnd-5e in catalog, got {ids:?}"
        );
        assert!(
            ids.contains(&"blades-in-the-dark"),
            "expected blades-in-the-dark in catalog, got {ids:?}"
        );
        // freeform was lifted out of the systems list into a top-level
        // `byo:` block in 2026-05-15; the wire-level magic slug went away.
        assert!(
            !ids.contains(&"freeform"),
            "freeform should not appear in the systems list anymore"
        );
        assert!(
            !raw.byo.bundle.is_empty(),
            "byo bundle must include at least one template"
        );
    }

    #[test]
    fn resolves_to_english_by_default_and_includes_bundle_metadata() {
        let raw = Catalog::load_from_embedded().unwrap();
        let cat = Catalog::from_raw(&raw, "en");
        let dnd = cat
            .systems
            .iter()
            .find(|s| s.id == "dnd-5e")
            .expect("dnd-5e present");
        assert_eq!(dnd.name, "D&D 5e (2014)");
        assert!(
            !dnd.bundle.is_empty(),
            "bundle must include at least one template"
        );
        let npc = dnd
            .bundle
            .iter()
            .find(|t| t.slug == "common/npc")
            .expect("common/npc in dnd-5e bundle");
        assert_eq!(npc.name, "NPC");
        assert_eq!(npc.icon, "person-standing");

        // BYO carries only the resolved bundle; its display copy lives in
        // the wizard frontend.
        assert!(!cat.byo.bundle.is_empty());
    }

    #[test]
    fn unknown_locale_falls_back_to_english() {
        let raw = Catalog::load_from_embedded().unwrap();
        let cat = Catalog::from_raw(&raw, "de");
        let dnd = cat.systems.iter().find(|s| s.id == "dnd-5e").unwrap();
        assert_eq!(dnd.name, "D&D 5e (2014)");
        // BYO bundle templates carry the same locale fallback as the rest
        // of the catalog.
        assert!(!cat.byo.bundle.is_empty());
    }
}
