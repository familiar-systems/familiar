//! Pure domain algebras for CRDT operations.
//!
//! [`CrdtDoc`](doc::CrdtDoc) is the data algebra: apply updates, export/import snapshots,
//! report a version vector. [`CrdtRoom`](room::CrdtRoom) is the domain algebra: membership,
//! dispatch, broadcast policy. Both are wire-format-agnostic and framework-agnostic (no kameo,
//! no Loro wrappers). Concrete Loro-backed implementations live in [`crate::loro`]. Wire-protocol
//! concerns (fragmentation, reassembly) live in [`crate::wire`].

pub mod doc;
pub mod room;
