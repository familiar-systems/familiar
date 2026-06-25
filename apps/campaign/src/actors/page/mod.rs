//! `PageActor`: per-Page CRDT room actor.
//!
//! Facade module. The implementation lives in [`page_actor`]; these
//! re-exports preserve the `crate::actors::page::*` import surface.

mod page_actor;
#[cfg(test)]
mod tests;

pub use page_actor::*;
