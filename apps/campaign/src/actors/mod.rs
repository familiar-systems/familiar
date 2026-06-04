//! Per-campaign actor system.
//!
//! Two-level topology:
//! - [`CampaignRegistry`](registry::CampaignRegistry) holds the map of
//!   live campaigns and is spawned once per process by `main`.
//! - [`CampaignSupervisor`](supervisor::CampaignSupervisor) is spawned
//!   per active campaign by the registry; it owns the
//!   [`DatabaseWriteActor`](database_writer::DatabaseWriteActor) and the idle-eviction
//!   clock.
//!
//! Child room actors under the supervisor:
//! - [`TocActor`](toc::TocActor): singleton, eagerly spawned on checkout.
//! - [`ThingActor`](thing::ThingActor): per-Thing, lazily spawned on first room join.
//!
//! Future: AgentConversation, RelationshipGraph, CampaignVocabulary.

pub mod database_writer;
pub mod persist;
pub mod registry;
pub mod supervisor;
pub mod thing;
pub mod toc;
