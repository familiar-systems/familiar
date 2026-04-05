//! Campaign-scoped shared library.
//!
//! Types and infrastructure used exclusively by the campaign server.
//! The platform server does not depend on this crate.
//!
//! ## Modules
//!
//! - `loro`: Loro document layer (schema types, typed wrappers, CrdtDoc trait)
//! - `status`: Campaign view-status types (GM only, Player, Retconned)

pub mod id;
pub mod loro;
pub mod notification;
pub mod status;
