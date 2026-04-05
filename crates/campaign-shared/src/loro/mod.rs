//! Loro document layer: schema types, typed wrappers, and the CrdtDoc trait.
//!
//! Everything in this module exists because we use Loro as the CRDT engine.
//! Schema types define how data is laid out in Loro containers. Wrappers provide
//! typed access. PM constants define the `loro-prosemirror` interop convention.

pub mod prosemirror;
pub mod thing;
pub mod toc;

use serde_json::Value;

/// Newtype for a binary snapshot blob.
#[derive(Debug, Clone)]
pub struct Snapshot(pub Vec<u8>);

impl Snapshot {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Newtype for an encoded Loro version vector.
#[derive(Debug, Clone)]
pub struct VersionVector(pub Vec<u8>);

impl VersionVector {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Abstraction over a CRDT document.
///
/// Provides a uniform interface for snapshot import/export and update application
/// across document types (Thing, ToC, etc.). Actors hold `Box<dyn CrdtDoc>` or
/// concrete types depending on whether they need domain-specific methods.
///
/// Adapted from the loro-protocol reference server.
/// See: https://github.com/loro-dev/protocol/blob/main/rust/loro-websocket-server/src/lib.rs#L164
pub trait CrdtDoc: Send {
    /// Current version vector (oplog state).
    fn get_version(&self) -> VersionVector;

    /// Apply one or more CRDT updates from a peer.
    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), String>;

    /// Export the full document as a snapshot blob.
    fn export_snapshot(&self) -> Result<Snapshot, String>;

    /// Import a snapshot blob (used on startup to restore state).
    fn import_snapshot(&mut self, data: &Snapshot) -> Result<(), String>;

    /// Whether this document type supports persistence. Default: true.
    fn should_persist(&self) -> bool {
        true
    }

    /// Optional debug representation (e.g., for JSON dumps). Default: None.
    fn debug_value(&self) -> Option<Value> {
        None
    }
}
