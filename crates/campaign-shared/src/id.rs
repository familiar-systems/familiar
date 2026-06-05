//! Campaign-scoped ID types.
//!
//! These IDs identify entities that exist within a campaign. The platform server
//! never uses them directly.

use fs_id::{Ulid, fs_id};

/// Uniquely identifies a thing (NPC, location, item, etc.).
/// ULID for compact URLs (26 chars) and B-tree-friendly insert ordering.
#[fs_id(export_to = "types-campaign/src/generated/id/")]
pub struct ThingId(pub Ulid);

/// Uniquely identifies a block within a document. ULID gives approximate
/// chronological creation ordering and a single representation across
/// regular tables (TEXT primary key) and sqlite-vec virtual tables (which
/// only accept INTEGER or TEXT primary keys, not BLOB).
#[fs_id(export_to = "types-campaign/src/generated/id/")]
pub struct BlockId(pub Ulid);

/// Identifies a play session (discord call, table session, etc.).
#[fs_id(export_to = "types-campaign/src/generated/id/")]
pub struct SessionId(pub Ulid);

/// Identifies a specific suggestion made by an AI assistant.
#[fs_id(export_to = "types-campaign/src/generated/id/")]
pub struct SuggestionId(pub Ulid);

/// Identifies a specific conversation between one or more users and an AI assistant.
#[fs_id(export_to = "types-campaign/src/generated/id/")]
pub struct ConversationId(pub Ulid);

/// Identifies one connected client (one WebSocket upgrade -> one tab).
///
/// Server-minted at WS upgrade. Distinct from `UserId` (in `app-shared`):
/// a single user with two tabs has one `UserId` and two `ClientId`s. Routing
/// (which socket to send an ack to) keys on `ClientId`; authorship and
/// permissions key on `UserId`.
///
/// `u64` to match Loro's `PeerID` width. Whether a `ClientId` is *also* used
/// as the Loro peer id, or whether the client picks its own peer id, is a
/// CrdtRoom-level decision; the type is shape-compatible with both.
#[fs_id(export_to = "types-campaign/src/generated/id/")]
pub struct ClientId(pub u64);
