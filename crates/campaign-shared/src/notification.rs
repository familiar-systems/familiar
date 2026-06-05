//! Campaign-scoped notification types.
//!
//! These are pushed to connected clients over the WebSocket side-channel
//! (not via CRDT sync).

use crate::id::PageId;
use crate::loro::page::PageHandle;

#[allow(dead_code)]
struct PageCreated {
    id: PageId,
}

pub enum VocabularyNotification {
    PageCreated(PageHandle),
    PageRenamed(PageHandle),
    PageUpdated(PageHandle),
}

pub enum PageNotification {
    PersistenceDegraded(),
    PersistenceRestored(),
}
