//! `LocalizedString`: a BCP-47-keyed map with a guaranteed `en` entry.
//!
//! Mirrors the JSON Schema definition at
//! `content/.schemas/{systems,starter-content}-schema.json`: every translatable
//! field is an object whose keys are language tags and whose values are
//! strings. The schema requires `en`; we mirror that in code so `resolve`
//! can fall back without an Option.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalizedString(pub BTreeMap<String, String>);

impl LocalizedString {
    /// Resolves the string for `locale`, falling back to `en` if absent.
    /// Panics if `en` is also absent — the JSON Schema makes that case a
    /// build-time error, so encountering it at runtime means the schema
    /// was bypassed (a real invariant violation, not a missing default).
    pub fn resolve(&self, locale: &str) -> &str {
        if let Some(s) = self.0.get(locale) {
            return s;
        }
        // BCP-47 lookup: try the language part of `xx-YY` if the full tag
        // wasn't found (so `en-US` falls back to `en` before the global `en`).
        if let Some((lang, _)) = locale.split_once('-')
            && let Some(s) = self.0.get(lang)
        {
            return s;
        }
        self.0
            .get("en")
            .map(String::as_str)
            .expect("LocalizedString invariant: en is required by the JSON Schema")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ls(pairs: &[(&str, &str)]) -> LocalizedString {
        LocalizedString(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                .collect(),
        )
    }

    #[test]
    fn resolves_exact_locale_match() {
        let s = ls(&[("en", "hello"), ("fr", "bonjour")]);
        assert_eq!(s.resolve("fr"), "bonjour");
    }

    #[test]
    fn falls_back_to_en_for_unknown_locale() {
        let s = ls(&[("en", "hello")]);
        assert_eq!(s.resolve("de"), "hello");
    }

    #[test]
    fn falls_back_from_region_to_language() {
        let s = ls(&[("en", "hello"), ("fr", "bonjour")]);
        assert_eq!(s.resolve("fr-CA"), "bonjour");
    }

    #[test]
    fn region_match_wins_over_language_match() {
        let s = ls(&[("en", "hello"), ("fr", "bonjour"), ("fr-CA", "salut")]);
        assert_eq!(s.resolve("fr-CA"), "salut");
    }

    #[test]
    #[should_panic(expected = "LocalizedString invariant")]
    fn panics_when_en_missing_and_no_match() {
        let s = ls(&[("fr", "bonjour")]);
        let _ = s.resolve("de");
    }
}
