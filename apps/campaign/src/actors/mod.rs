//! Per-campaign actor system.
//!
//! Two-level topology:
//! - [`CampaignRegistry`](registry::CampaignRegistry) holds the map of
//!   live campaigns and is spawned once per process by `main`.
//! - [`CampaignSupervisor`](supervisor::CampaignSupervisor) is spawned
//!   per active campaign by the registry; it owns the
//!   [`DatabaseActor`](database_writer::DatabaseActor), the
//!   [`TocActor`](toc::TocActor), and any spawned
//!   [`ThingActor`](thing::ThingActor)s.

pub mod database_writer;
pub mod registry;
pub mod supervisor;
pub mod thing;
pub mod toc;

use kameo::actor::ActorRef;

/// Per-connection routing handle for CRDT rooms.
///
/// The WebSocket read task populates a `HashMap<String, RoomHandle>` on
/// `JoinRequest` and dispatches `DocUpdate` directly to actors via this
/// enum, bypassing the supervisor on the hot path.
///
/// Why an enum, not a trait object: `kameo::ActorRef<A>` is generic over
/// the concrete actor type. Can't erase to `ActorRef<dyn CrdtRoom>`.
pub enum RoomHandle {
    Toc(ActorRef<toc::TocActor>),
    Thing(ActorRef<thing::ThingActor>),
}

impl RoomHandle {
    pub async fn apply_updates(&self, conn_id: u64, updates: Vec<Vec<u8>>) {
        match self {
            RoomHandle::Toc(actor) => {
                let _ = actor
                    .tell(toc::ApplyUpdates { conn_id, updates })
                    .send()
                    .await;
            }
            RoomHandle::Thing(actor) => {
                let _ = actor
                    .tell(thing::ApplyUpdates { conn_id, updates })
                    .send()
                    .await;
            }
        }
    }

    pub async fn leave(&self, conn_id: u64) {
        match self {
            RoomHandle::Toc(actor) => {
                let _ = actor.tell(toc::ClientLeave { conn_id }).send().await;
            }
            RoomHandle::Thing(actor) => {
                let _ = actor.tell(thing::ClientLeave { conn_id }).send().await;
            }
        }
    }
}
