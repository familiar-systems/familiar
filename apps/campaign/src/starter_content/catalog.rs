//! Catalog parsing: `content/systems.yaml` and the templates each system bundles.
//!
//! The `content/` directory is embedded at build time via [`include_dir!`]:
//! a bundle slug that doesn't resolve to a `templates/<slug>/<leaf>.en.md`
//! file, a template with malformed frontmatter, or a body that fails to compile
//! to blocks all fail the test that constructs the catalog (see
//! [`Catalog::load_from_embedded`]). There is no runtime path that returns
//! "template not found" or "template won't compile"; by the time `axum::serve`
//! runs, the catalog is either fully resolved or the binary aborted.
//!
//! A template is a single markdown file per locale (`<leaf>.<locale>.md`); its
//! frontmatter carries the catalog-card metadata and its body is the block
//! content the import compiler materializes. `systems.yaml` stays YAML: it is
//! system config (slugs, colors, labels), not template content.

use crate::starter_content::{
    compile,
    localized::LocalizedString,
    template::{Frontmatter, split_frontmatter},
};
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

/// One template's markdown files, keyed by locale. Frontmatter is parsed at
/// load (fail-fast on malformed content); the raw body is compiled to blocks
/// on demand at import time.
#[derive(Debug, Clone)]
struct RawTemplate {
    by_locale: BTreeMap<String, LocaleTemplate>,
}

#[derive(Debug, Clone)]
struct LocaleTemplate {
    markdown: String,
    frontmatter: Frontmatter,
}

impl RawTemplate {
    /// The best locale match, falling back to `en` (guaranteed present at load).
    fn resolve(&self, locale: &str) -> &LocaleTemplate {
        resolve_locale(&self.by_locale, locale).expect("en locale guaranteed at load time")
    }
}

/// BCP-47 locale lookup shared by templates: exact tag, then the language part
/// of `xx-YY`, then the required `en` baseline.
fn resolve_locale<'a, T>(map: &'a BTreeMap<String, T>, locale: &str) -> Option<&'a T> {
    if let Some(v) = map.get(locale) {
        return Some(v);
    }
    if let Some((lang, _)) = locale.split_once('-')
        && let Some(v) = map.get(lang)
    {
        return Some(v);
    }
    map.get("en")
}

impl Catalog {
    /// Loads the embedded catalog. Returns an error string at startup time
    /// (and from tests) if `systems.yaml` references a slug that doesn't
    /// resolve to a `templates/<slug>/<leaf>.en.md` file or whose frontmatter
    /// is malformed.
    pub fn load_from_embedded() -> Result<RawCatalog, String> {
        let systems_yaml = CONTENT_DIR
            .get_file("systems.yaml")
            .ok_or("content/systems.yaml not found in embedded directory")?
            .contents_utf8()
            .ok_or("content/systems.yaml is not valid UTF-8")?;
        let parsed: SystemsYaml =
            serde_yaml::from_str(systems_yaml).map_err(|e| format!("content/systems.yaml: {e}"))?;

        let mut templates: BTreeMap<String, RawTemplate> = BTreeMap::new();

        // Bundle slugs from real systems + the BYO bundle all share the
        // same `templates/` directory; one resolver walks both.
        let system_slug_sources = parsed.systems.iter().map(|s| (s.id.as_str(), &s.bundle));
        let byo_slug_source = std::iter::once(("byo", &parsed.byo.bundle));
        for (origin, bundle) in system_slug_sources.chain(byo_slug_source) {
            for slug in bundle {
                if templates.contains_key(slug) {
                    continue;
                }
                templates.insert(slug.clone(), load_template(slug, origin)?);
            }
        }

        Ok(RawCatalog {
            systems: parsed.systems,
            byo: parsed.byo,
            templates,
        })
    }

    /// Resolves a [`RawCatalog`] for `locale`, falling back to `en` per
    /// [`resolve_locale`] / [`LocalizedString::resolve`].
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

/// Load a bundled template's per-locale markdown files from
/// `templates/<slug>/<leaf>.<locale>.md`, parsing each file's frontmatter. The
/// slug's last path segment is the `<leaf>` filename stem.
fn load_template(slug: &str, origin: &str) -> Result<RawTemplate, String> {
    let leaf = slug.rsplit('/').next().unwrap_or(slug);
    let dir_path = format!("templates/{slug}");
    let dir = CONTENT_DIR
        .get_dir(&dir_path)
        .ok_or_else(|| format!("content/{dir_path}/ not found (referenced by {origin} bundle)"))?;

    let prefix = format!("{leaf}.");
    let mut by_locale: BTreeMap<String, LocaleTemplate> = BTreeMap::new();
    for file in dir.files() {
        let Some(name) = file.path().file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // `<leaf>.<locale>.md` -> locale.
        let Some(locale) = name
            .strip_prefix(&prefix)
            .and_then(|rest| rest.strip_suffix(".md"))
            .filter(|locale| !locale.is_empty())
        else {
            continue;
        };
        let markdown = file
            .contents_utf8()
            .ok_or_else(|| format!("content/{dir_path}/{name} is not valid UTF-8"))?;
        let (frontmatter, _body) =
            split_frontmatter(markdown).map_err(|e| format!("content/{dir_path}/{name}: {e}"))?;
        by_locale.insert(
            locale.to_string(),
            LocaleTemplate {
                markdown: markdown.to_string(),
                frontmatter,
            },
        );
    }

    let Some(en) = by_locale.get("en") else {
        return Err(format!(
            "content/{dir_path}/ has no `{leaf}.en.md` (the required baseline, referenced by {origin} bundle)"
        ));
    };
    // Fail-fast: a bundled template whose body can't compile aborts startup, not
    // a campaign creation (where seeding is best-effort and would silently skip
    // it). Upholds the module's "malformed fails the build" invariant.
    compile::compile_template(&en.markdown)
        .map_err(|e| format!("content/{dir_path}/{leaf}.en.md body: {e}"))?;

    Ok(RawTemplate { by_locale })
}

fn resolve_bundle(
    slugs: &[String],
    templates: &BTreeMap<String, RawTemplate>,
    locale: &str,
) -> Vec<TemplateRef> {
    slugs
        .iter()
        .map(|slug| {
            let fm = &templates
                .get(slug)
                .expect("validated at load time")
                .resolve(locale)
                .frontmatter;
            TemplateRef {
                slug: slug.clone(),
                name: fm.name.clone(),
                description: fm.description.clone(),
                icon: fm.icon.clone(),
            }
        })
        .collect()
}

/// Locale-unresolved catalog. Held in `AppState` and resolved per-request.
#[derive(Debug, Clone)]
pub struct RawCatalog {
    systems: Vec<SystemRowYaml>,
    byo: ByoRowYaml,
    templates: BTreeMap<String, RawTemplate>,
}

impl RawCatalog {
    /// The raw markdown of a bundled template at `locale` (en fallback), for the
    /// import compiler. `None` if the slug is not in any bundle (so was never
    /// loaded). The slug space is closed at load time, so a `None` here is a
    /// caller passing a slug the catalog never advertised.
    pub fn template_markdown(&self, slug: &str, locale: &str) -> Option<&str> {
        self.templates
            .get(slug)
            .map(|t| t.resolve(locale).markdown.as_str())
    }
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
        // Metadata now comes from the markdown frontmatter, not a sidecar YAML.
        assert_eq!(npc.name, "NPC");
        assert_eq!(npc.icon, "contact");

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
        // Template bundles carry the same locale fallback as the rest of the
        // catalog: the `en` markdown resolves under an unknown locale.
        let npc = dnd.bundle.iter().find(|t| t.slug == "common/npc").unwrap();
        assert_eq!(npc.name, "NPC");
        assert!(!cat.byo.bundle.is_empty());
    }

    #[test]
    fn template_markdown_available_for_bundled_slug() {
        let raw = Catalog::load_from_embedded().unwrap();
        let md = raw
            .template_markdown("common/npc", "en")
            .expect("common/npc markdown available for import");
        assert!(md.contains("<player_visible>"), "body should be present");
        assert!(
            raw.template_markdown("common/does-not-exist", "en")
                .is_none(),
            "an unbundled slug has no markdown"
        );
    }
}
