//! A template file on disk: YAML frontmatter followed by a markdown body.
//!
//! One file per locale (`<leaf>.<locale>.md`), so the frontmatter fields are
//! plain scalars, not per-locale maps - localization is by filename. The
//! frontmatter is the template's catalog-card identity (name, description,
//! icon); the body is the block content the import compiler turns into a page.
//! This module owns only the split and the frontmatter shape; the body compiler
//! lives in [`compile`].
//!
//! The `onCreate` block (the tag applied when an entity is created *from* a
//! template) is intentionally not modeled here: instantiation is not built yet,
//! so parsing it would be speculative. serde ignores the unknown key until the
//! instantiation slice needs it.
//!
//! [`compile`]: crate::starter_content::compile

use serde::Deserialize;

/// The `---`-fenced YAML block at the top of a template file.
#[derive(Debug, Clone, Deserialize)]
pub struct Frontmatter {
    /// Display name of the template and of entities created from it.
    pub name: String,
    /// One-line catalog-card description.
    pub description: String,
    /// `lucide-react` icon name for the catalog card.
    pub icon: String,
}

/// Split a template file into its parsed frontmatter and the raw markdown body.
///
/// The file must begin with a `---` fence line, hold a YAML block, and close
/// with a `---` fence line; everything after the closing fence is the body
/// (returned verbatim, leading blank line included). A malformed file is an
/// error, not a silent empty template - the caller fails the build/startup.
pub fn split_frontmatter(md: &str) -> Result<(Frontmatter, &str), String> {
    let md = md.strip_prefix('\u{feff}').unwrap_or(md);
    let after_open = md
        .strip_prefix("---\n")
        .or_else(|| md.strip_prefix("---\r\n"))
        .ok_or("must begin with a `---` frontmatter fence")?;

    let (yaml, body) =
        split_at_closing_fence(after_open).ok_or("frontmatter is not closed by a `---` line")?;
    let frontmatter: Frontmatter =
        serde_yaml::from_str(yaml).map_err(|e| format!("frontmatter: {e}"))?;
    Ok((frontmatter, body))
}

/// Find the first line that is exactly `---` and split there: everything before
/// it is the YAML, everything after that line is the body.
fn split_at_closing_fence(s: &str) -> Option<(&str, &str)> {
    let mut offset = 0;
    for line in s.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            return Some((&s[..offset], &s[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const NPC: &str = "---\nname: NPC\ndescription: A character.\nicon: contact\nonCreate:\n    tag: NPC\n---\n\n<player_visible>\nWho is this?\n</player_visible>\n";

    #[test]
    fn splits_frontmatter_and_body_ignoring_on_create() {
        let (fm, body) = split_frontmatter(NPC).unwrap();
        assert_eq!(fm.name, "NPC");
        assert_eq!(fm.icon, "contact");
        // `onCreate` is present in the file but unmodeled; parsing tolerates it.
        assert!(body.contains("<player_visible>"));
        assert!(!body.contains("name: NPC"), "body must exclude frontmatter");
    }

    #[test]
    fn rejects_missing_opening_fence() {
        assert!(split_frontmatter("name: NPC\n").is_err());
    }

    #[test]
    fn rejects_unclosed_frontmatter() {
        assert!(split_frontmatter("---\nname: NPC\n").is_err());
    }

    #[test]
    fn rejects_frontmatter_missing_required_field() {
        // No `icon` -> serde error, surfaced as a build/startup failure.
        let md = "---\nname: NPC\ndescription: A character.\n---\nbody\n";
        assert!(split_frontmatter(md).is_err());
    }
}
