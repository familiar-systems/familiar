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
//! Child actors under the supervisor:
//! - [`TocActor`](toc::TocActor): CRDT singleton, eagerly spawned on checkout.
//! - [`RelationshipGraph`](relationship_graph::RelationshipGraph): server-
//!   authoritative graph singleton (not a CRDT room), eagerly spawned on checkout.
//! - [`PageActor`](page::PageActor): CRDT-per-Page, lazily spawned on first room join.
//!
//! Future: AgentConversation, CampaignVocabulary.

pub mod database_writer;
pub mod page;
pub mod persist;
pub mod registry;
pub mod relationship_graph;
pub mod supervisor;
pub mod toc;
