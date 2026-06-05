//! WebSocket layer for CRDT room sync via the loro-protocol.
//!
//! One WebSocket per campaign per client. Each connection spawns a
//! read/write task pair. The read task holds a local routing table
//! for hot-path DocUpdate dispatch. The supervisor is only in the
//! path for JoinRequest and disconnect.
//!
//! See the [campaign actor domain design](../../../docs/plans/2026-05-04-campaign-actor-domain-design.md)
//! for the full architecture.

pub mod connection;
pub mod upgrade;
