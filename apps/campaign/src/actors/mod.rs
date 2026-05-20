//! Per-campaign actor system.
//!
//! Two-level topology:
//! - [`CampaignRegistry`](registry::CampaignRegistry) holds the map of
//!   live campaigns and is spawned once per process by `main`.
//! - [`CampaignSupervisor`](supervisor::CampaignSupervisor) is spawned
//!   per active campaign by the registry; it owns the
//!   [`DatabaseActor`](database::DatabaseActor) and the idle-eviction
//!   clock.
//!
//! Future child room actors (ThingActor, TocActor, AgentConversation,
//! relationship graph, vocabulary) attach under the supervisor. At the
//! time of writing none of them exist yet.

pub mod database_writer;
pub mod registry;
pub mod supervisor;
