//! CRDT document algebra.
//!
//! `CrdtDoc` is the algebra of CRDT operations on a single document. Concrete
//! Loro-backed implementations live in `crate::loro` (`LoroPageDoc`,
//! `LoroTocDoc`, etc). The trait is the contract every doc-shaped actor's
//! inner state satisfies.

/// Newtype for a binary snapshot blob exported by the underlying CRDT.
#[derive(Debug, Clone)]
pub struct Snapshot(pub Vec<u8>);

impl Snapshot {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Newtype for an encoded CRDT version vector.
#[derive(Debug, Clone, PartialEq)]
pub struct VersionVector(pub Vec<u8>);

impl VersionVector {
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Failure modes for CRDT byte operations: the `CrdtDoc` trait methods plus the
/// ToC's tree edits, which export an update delta to broadcast.
///
/// Each variant carries the underlying error description (Loro's `LoroError`
/// formatted into a String). The variant tells the caller *which* operation
/// failed; the inner string carries the underlying detail for logging.
#[derive(Debug, thiserror::Error)]
pub enum DocError {
    #[error("apply update failed: {0}")]
    ApplyUpdate(String),
    #[error("export snapshot failed: {0}")]
    ExportSnapshot(String),
    #[error("export updates failed: {0}")]
    ExportUpdates(String),
}

/// CRDT algebra. Implemented by inner Loro-backed types (e.g. `LoroPageDoc`,
/// `LoroTocDoc`) and consumed by actor-side message handlers, the persistence
/// pipeline, and tests.
///
/// Adapted from the loro-protocol reference server:
/// <https://github.com/loro-dev/protocol/blob/main/rust/loro-websocket-server/src/lib.rs>
pub trait CrdtDoc: Send {
    /// Current version vector (oplog state).
    fn version(&self) -> VersionVector;

    /// Apply one or more CRDT updates from a peer.
    fn apply_updates(&mut self, updates: &[Vec<u8>]) -> Result<(), DocError>;

    /// Export the full document as a snapshot blob.
    fn export_snapshot(&self) -> Result<Snapshot, DocError>;

    /// Whether this document type wants to participate in the snapshot
    /// persistence pipeline. Default: true. Doc types that derive their
    /// state from elsewhere can return false.
    fn should_persist(&self) -> bool {
        true
    }

    /// Optional debug representation (e.g. for JSON dumps in dev tooling).
    /// Default: None.
    fn debug_value(&self) -> Option<serde_json::Value> {
        None
    }
}
