//! Catalog parser for `content/systems.yaml` and `content/templates/**/*.yaml`.
//!
//! v0 scope: parse YAML at startup, expose locale-resolved system + template
//! metadata for the catalog endpoint. The full design's body parser and Loro
//! compiler land in a later slice.
//!
//! Content directory is embedded at build time via [`include_dir`] so a
//! deploy can't drift from the binary's expected layout, and a missing /
//! malformed template fails the build, not a request.

pub mod catalog;
pub mod localized;
pub mod template;

pub use catalog::{Catalog, RawCatalog};
pub use localized::LocalizedString;
