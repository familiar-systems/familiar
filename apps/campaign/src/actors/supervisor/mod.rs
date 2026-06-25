//! `CampaignSupervisor`: per-campaign orchestrator.
//!
//! Facade module. The actor, lifecycle, and idle/health handlers live in
//! `supervisor_actor`; page/session creation in `creation`; WebSocket room
//! routing in `routing`; metadata + read-side queries in `queries`.
//! Re-exports preserve the `crate::actors::supervisor::*` import surface.

mod creation;
mod queries;
mod routing;
mod supervisor_actor;
#[cfg(test)]
mod tests;

pub use creation::*;
pub use queries::*;
pub use routing::*;
pub use supervisor_actor::*;
