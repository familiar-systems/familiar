pub enum CrdtRoomType {
    Thing,
    Toc,
    Conversation,
}

/// A CRDT room is a doc that clients can join, exchange updates against, and leave.
/// Implemented by inner-state types (e.g. ThingRoom).
pub trait CrdtRoom {
    fn room_id(&self) -> &str;
    fn crdt_room_type(&self) -> CrdtRoomType;
    fn on_join(&mut self, client: ClientId, auth: &[u8]) -> Result<JoinResponse, JoinError>;
    /// Apply one or more updates from a client to the CRDT state.
    fn apply_updates(
        &mut self,
        from: ClientId,
        updates: &[Vec<u8>],
    ) -> Result<(Broadcast, Ack), UpdateError>;
    fn on_leave(&mut self, client: ClientId);
}
