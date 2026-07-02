//! Starter content: `content/systems.yaml` and the per-locale template markdown
//! files each system bundles (`content/templates/<slug>/<leaf>.<locale>.md`).
//!
//! Startup parses `systems.yaml` and each bundled template's frontmatter, and
//! exposes locale-resolved metadata for the catalog endpoint plus the raw
//! template markdown for the import [`compile`]r. The `content/` directory is
//! embedded at build time via [`include_dir`] so a deploy can't drift from the
//! binary's layout, and a missing / malformed template fails the build, not a
//! request.
//!
//! [`compile`]: crate::starter_content::compile

pub mod catalog;
pub mod compile;
pub mod localized;
pub mod template;

pub use catalog::{Catalog, RawCatalog};
pub use localized::LocalizedString;
