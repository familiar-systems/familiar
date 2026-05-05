//! Campaign-scoped shared library.
//!
//! Holds types that cross the language boundary (consumed by `types-campaign`
//! via ts-rs) or coordinate between the platform and campaign servers. Pure
//! Rust behaviour without an external consumer (CRDT wrappers, the `CrdtDoc`
//! trait, persistence, actors) lives in `apps/campaign`, not here.
//!
//! ## Modules
//!
//! - `id`: Branded ID newtypes (ts-rs exported).
//! - `loro`: Loro doc schema: container/key constants and ts-rs-exported
//!   entry types (`ThingHandle`, `TocEntry`, etc).
//! - `notification`: WebSocket side-channel notification types.
//! - `status`: Campaign view-status enum (GM only, Known, Retconned).

pub mod id;
pub mod loro;
pub mod notification;
pub mod status;
