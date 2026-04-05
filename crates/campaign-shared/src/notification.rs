//! Campaign-scoped notification types.
//!
//! These are pushed to connected clients over the WebSocket side-channel
//! (not via CRDT sync).

use crate::id::ThingId;
use crate::loro::thing::ThingHandle;

#[allow(dead_code)]
struct ThingCreated {
    id: ThingId,
}

pub enum VocabularyNotification {
    ThingCreated(ThingHandle),
    ThingRenamed(ThingHandle),
    ThingUpdated(ThingHandle),
}

pub enum ThingNotification {
    PersistenceDegraded(),
    PersistenceRestored(),
}
