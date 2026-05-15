//! Per-template YAML deserialization.
//!
//! v0 only reads `meta` (name + description + icon). The `body` field is
//! deserialized into `serde_yaml::Value` and ignored at runtime — the catalog
//! endpoint never returns body content. The body parser and Loro compiler
//! land in a later slice; keeping `body` permissively typed here means a
//! template with a future widget node still parses today and is ignored.

use crate::starter_content::localized::LocalizedString;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Template {
    pub meta: TemplateMeta,
    /// Ignored in v0; kept for forward-compat with the body parser.
    #[serde(default)]
    #[allow(dead_code)]
    pub body: serde_yaml::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TemplateMeta {
    pub name: LocalizedString,
    pub description: LocalizedString,
    pub icon: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NPC_YAML: &str = r#"
meta:
  name:
    en: NPC
  description:
    en: A character.
  icon: person-standing
body:
  - node: paragraph
    text:
      en: hello
"#;

    #[test]
    fn parses_template_meta_and_ignores_body() {
        let t: Template = serde_yaml::from_str(NPC_YAML).unwrap();
        assert_eq!(t.meta.name.resolve("en"), "NPC");
        assert_eq!(t.meta.icon, "person-standing");
    }

    #[test]
    fn rejects_meta_with_missing_en() {
        let bad = r#"
meta:
  name:
    fr: PNJ
  description:
    en: A character.
  icon: person-standing
body: []
"#;
        // Parses (BTreeMap<String,String> doesn't enforce 'en'), but the
        // resolve invariant fires when the catalog tries to use it. The
        // JSON Schema is what enforces 'en' at content-author time; this
        // assertion locks the runtime fallback behavior.
        let t: Template = serde_yaml::from_str(bad).unwrap();
        let result = std::panic::catch_unwind(|| t.meta.name.resolve("de").to_string());
        assert!(result.is_err(), "expected panic on missing en");
    }
}
