//! `TocActor`: the campaign's table-of-contents CRDT room.
//!
//! Facade module. The actor lives in `toc_actor`; the pure row<->tree
//! serialization in `toc_snapshot`. Re-exports preserve the
//! `crate::actors::toc::*` import surface.

#[cfg(test)]
mod tests;
mod toc_actor;
mod toc_snapshot;

pub use toc_actor::*;
pub use toc_snapshot::*;
