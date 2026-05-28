//! Actor-facing contract for CRDT rooms.
//!
//! Contains the kameo `Message` structs that the WebSocket layer sends to
//! room-capable actors (TocActor, ThingActor, AgentConversation), the reply
//! and result types those handlers return, and shared enums (`Capability`,
//! error types) that flow across the actor boundary.
//!
//! The separation from [`super::room`] is intentional. [`Room<D>`](super::room::Room)
//! is pure orchestration (subscriber tracking, broadcast computation,
//! snapshot-on-join) that consumes these types. These types define the contract
//! every room actor satisfies; `Room` is one implementation of that contract.

use familiar_systems_app_shared::campaigns::internal::CampaignRole;
use familiar_systems_campaign_shared::id::ClientId;
use tokio::sync::mpsc;

use super::doc::{Snapshot, VersionVector};

// ---------------------------------------------------------------------------
// Capability
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Messages (inbound to actor)
// ---------------------------------------------------------------------------

/// A WebSocket client wants to join this room. The actor resolves the
/// pre-validated [`CampaignRole`] into a [`Capability`] based on room-level
/// policy before calling [`Room::on_join`](super::room::Room::on_join).
///
/// The role is established at WebSocket upgrade time (Hanko validation +
/// platform membership check), not per-JoinRequest. The loro-protocol
/// `JoinRequest.auth` bytes are ignored server-side.
#[derive(Debug)]
pub struct ClientJoin {
    pub client: ClientId,
    pub tx: mpsc::UnboundedSender<Vec<u8>>,
    pub role: CampaignRole,
}

/// A WebSocket client disconnected or sent a Leave frame. The actor removes
/// the subscriber from the room and may start its idle-eviction timer if no
/// subscribers remain.
#[derive(Debug)]
pub struct ClientLeave {
    pub client: ClientId,
}

/// CRDT update chunks from a WebSocket client (one `DocUpdate` on the wire).
/// The actor delegates to [`Room::apply_updates`](super::room::Room::apply_updates),
/// then marks itself dirty and arms the persistence debounce.
#[derive(Debug)]
pub struct ClientUpdate {
    pub client: ClientId,
    pub updates: Vec<Vec<u8>>,
}

// ---------------------------------------------------------------------------
// Replies and results (outbound from actor / room)
// ---------------------------------------------------------------------------

/// Snapshot + permission handed back to a freshly-joined client. The actor
/// encodes this into `ProtocolMessage::JoinResponseOk`.
#[derive(Debug)]
pub struct JoinResponse {
    /// Encoded full-document snapshot (loro `export(Snapshot)`).
    pub snapshot: Snapshot,
    /// Server's current oplog version vector at join time.
    pub version: VersionVector,
    /// Coarse capability gate for this socket (`Read` or `Write`). Domain
    /// authorization (GM vs player, gm_only blocks) lives in the actor's
    /// message handler, not here; this is only the wire-level handshake.
    pub permission: Capability,
}

#[derive(Debug, thiserror::Error)]
pub enum JoinError {
    #[error("room full")]
    Full,
    #[error("internal: {0}")]
    Internal(String),
}

/// CRDT updates to fan out to other subscribers in the same room. Carries
/// the same `Vec<Vec<u8>>` shape as the wire-level DocUpdate so the actor's
/// broadcast loop is a thin re-encode.
#[derive(Debug, Clone)]
pub struct Broadcast {
    pub updates: Vec<Vec<u8>>,
    /// `Some(c)` skips subscriber `c` when fanning out (the originator,
    /// who sees their own write echoed back via the `Ack` rather than the
    /// broadcast). `None` broadcasts to all subscribers, used when the room
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
