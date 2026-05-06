use super::doc::{Snapshot, VersionVector};
use familiar_systems_campaign_shared::id::ClientId;

pub enum CrdtRoomType {
    Thing,
    Toc,
    Conversation,
}

/// Closely mirrors
/// - loro_protocol::Permission
/// - a similar Yrs construct.
///
/// Maps to `Read`/`Write` capabilities on the wire.
/// Kept as a separate enum to avoid coupling domain with Loro.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Read,
    Write,
}

/// A CRDT room is a doc that clients can join, exchange updates against, and leave.
/// Implemented by inner-state types (e.g. ThingRoom).
pub trait CrdtRoom {
    fn room_id(&self) -> &str;
    fn crdt_room_type(&self) -> CrdtRoomType;
    fn on_join(&mut self, client: ClientId, auth: &[u8]) -> Result<JoinResponse, JoinError>;
    /// Apply one or more updates from a client. Returns a uniform broadcast
    /// (fanned out to other subscribers) plus an `AckPayload` carrying the
    /// post-apply version. The actor wraps the version into the wire-level
    /// `ProtocolMessage::Ack` along with the originating `batch_id` and
    /// status byte; correlation is the actor's concern, not the room's.
    fn apply_updates(
        &mut self,
        from: ClientId,
        updates: &[Vec<u8>],
    ) -> Result<(Broadcast, AckPayload), UpdateError>;
    fn on_leave(&mut self, client: ClientId);
}

/// Snapshot + permission handed back to a freshly-joined client. The actor
/// encodes this into `ProtocolMessage::JoinResponseOk`.
#[derive(Debug, Clone)]
pub struct JoinResponse {
    /// Encoded full-document snapshot (loro `export(Snapshot)`).
    pub snapshot: Snapshot,
    /// Server's current oplog version vector at join time.
    pub version: VersionVector,
    /// Coarse capability gate for this socket (`Read` or `Write`). Domain
    /// authorization (GM vs player, gm_only blocks) lives in `apply_updates`,
    /// not here; this is only the wire-level handshake.
    pub permission: Capability,
}

#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("room not found")]
    NotFound,
    #[error("internal: {0}")]
    Internal(String),
}

/// CRDT updates to fan out to other subscribers in the same room. Carries
/// the same `Vec<Vec<u8>>` shape as the wire-level DocUpdate so the actor's
/// broadcast loop is a thin re-encode.
#[derive(Debug, Clone)]
pub struct Broadcast {
    pub updates: Vec<Vec<u8>>,
    /// `Some(c)` skips subscriber `c` when fanning out — the originator,
    /// who sees their own write echoed back via the `Ack` rather than the
    /// broadcast. `None` broadcasts to all subscribers, used when the room
    /// produced cascade-style edits the originator hasn't seen yet
    /// (idempotent under CRDT semantics, so duplicate delivery is harmless).
    pub exclude: Option<ClientId>,
}

/// Per-sender acknowledgment that a batch was applied. The actor encodes
/// this into `ProtocolMessage::Ack` and sends it to the originating client.
#[derive(Debug, Clone)]
pub struct AckPayload {
    /// Server's oplog version vector after applying the batch.
    pub version: VersionVector,
}

#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("unauthorized write")]
    Unauthorized,
    #[error("crdt apply failed: {0}")]
    Apply(String),
    #[error("invalid update payload: {0}")]
    Invalid(String),
}
