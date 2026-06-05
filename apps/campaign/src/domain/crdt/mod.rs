//! Pure domain algebras for CRDT operations.
//!
//! [`CrdtDoc`](doc::CrdtDoc) is the data algebra: apply updates, export/import snapshots,
//! report a version vector. [`Room`](room::Room) is the orchestration layer: subscriber
//! management, snapshot-on-join, broadcast computation. `CrdtDoc` is wire-format-agnostic
//! and framework-agnostic (no kameo, no Loro wrappers). `Room` depends on tokio channels
//! for subscriber delivery but not on kameo or the wire protocol. Concrete Loro-backed
//! doc implementations live in [`crate::loro`]. Wire-protocol concerns (fragmentation,
//! reassembly) live in [`crate::wire`].

pub mod doc;
pub mod room;
pub mod room_actor;
