//! `RelationshipGraph`: server-authoritative relationship graph actor.
//!
//! Facade module. The actor lives in [`relationship_graph_actor`]; the in-memory
//! store + row conversion in [`relationship_graph_store`]. Re-exports preserve
//! the `crate::actors::relationship_graph::*` import surface.

mod relationship_graph_actor;
mod relationship_graph_store;

pub use relationship_graph_actor::*;
