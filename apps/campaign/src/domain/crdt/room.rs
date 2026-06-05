use std::collections::HashMap;

use super::doc::CrdtDoc;
use super::room_actor::{AckPayload, Broadcast, Capability, JoinError, JoinResponse, UpdateError};
use familiar_systems_campaign_shared::id::ClientId;
use tokio::sync::mpsc;

pub enum CrdtRoomType {
    Thing,
    Toc,
    Conversation,
}

struct Subscriber {
    tx: mpsc::UnboundedSender<Vec<u8>>,
    capability: Capability,
}

/// Generic CRDT room: subscriber management, snapshot-on-join, broadcast
/// computation. Parameterized over a [`CrdtDoc`] for the content layer.
///
/// Authorization is the actor's concern. The actor resolves auth bytes into
/// a [`Capability`] and passes the result here. Room never sees raw auth.
pub struct Room<D: CrdtDoc> {
    doc: D,
    subscribers: HashMap<ClientId, Subscriber>,
}

impl<D: CrdtDoc> Room<D> {
    pub fn new(doc: D) -> Self {
        Self {
            doc,
            subscribers: HashMap::new(),
        }
    }

    /// Access the inner doc (e.g. for actor-level reads or snapshots).
    pub fn doc(&self) -> &D {
        &self.doc
    }

    /// Mutable access to the inner doc (e.g. for actor-level mutations
    /// like suggestion injection or domain-event-driven updates).
    pub fn doc_mut(&mut self) -> &mut D {
        &mut self.doc
    }

    /// Register a subscriber with a pre-resolved capability. Exports a
    /// snapshot for the joining client. Auth is the caller's responsibility.
    pub fn on_join(
        &mut self,
        client: ClientId,
        tx: mpsc::UnboundedSender<Vec<u8>>,
        capability: Capability,
    ) -> Result<JoinResponse, JoinError> {
        let snapshot = self
            .doc
            .export_snapshot()
            .map_err(|e| JoinError::Internal(e.to_string()))?;
        let version = self.doc.version();
        self.subscribers
            .insert(client, Subscriber { tx, capability });
        Ok(JoinResponse {
            snapshot,
            version,
            permission: capability,
        })
    }

    /// Apply updates from a client. Returns the broadcast payload (for
    /// fan-out to other subscribers) and an ack payload (for the sender).
    ///
    /// The actor wraps the ack into `ProtocolMessage::Ack` with the
    /// originating `batch_id`; correlation is the actor's concern.
    pub fn apply_updates(
        &mut self,
        from: ClientId,
        updates: &[Vec<u8>],
    ) -> Result<(Broadcast, AckPayload), UpdateError> {
        // Fail closed: only a registered Write subscriber may apply updates. An
        // absent `from` (never joined, or a ref to a different actor instance)
        // is rejected, not silently applied. Server-originated edits bypass
        // this path (they go through `doc_mut()`), so the only legitimate
        // caller here is a joined GM/Write client.
        let authorized = matches!(
            self.subscribers.get(&from),
            Some(Subscriber {
                capability: Capability::Write,
                ..
            })
        );
        if !authorized {
            return Err(UpdateError::Unauthorized);
        }

        self.doc
            .apply_updates(updates)
            .map_err(|e| UpdateError::Apply(e.to_string()))?;

        let version = self.doc.version();
        Ok((
            Broadcast {
                updates: updates.to_vec(),
                exclude: Some(from),
            },
            AckPayload { version },
        ))
    }

    /// Fan out opaque byte frames to subscribers, skipping `exclude`.
    ///
    /// Room owns subscriber channels but has no wire knowledge. The actor
    /// encodes `Broadcast` updates into `ProtocolMessage` frames (using
    /// `wire::BatchFragmenter` for payloads over 256KB), then calls this
    /// to distribute the encoded frames. Room just delivers bytes.
    pub fn fan_out(&self, frames: &[Vec<u8>], exclude: Option<ClientId>) {
        for (id, sub) in &self.subscribers {
            if exclude == Some(*id) {
                continue;
            }
            for frame in frames {
                let _ = sub.tx.send(frame.clone());
            }
        }
    }

    pub fn on_leave(&mut self, client: ClientId) {
        self.subscribers.remove(&client);
    }

    pub fn subscriber_count(&self) -> usize {
        self.subscribers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::crdt::doc::{DocError, Snapshot, VersionVector};

    /// Minimal in-memory [`CrdtDoc`] that records how many times
    /// `apply_updates` ran. Keeps these tests about the authorization guard
    /// rather than Loro byte encoding, and lets each case assert the doc is
    /// left untouched when a write is rejected.
    #[derive(Default)]
    struct StubDoc {
        applied: usize,
    }

    impl CrdtDoc for StubDoc {
        fn version(&self) -> VersionVector {
            VersionVector(Vec::new())
        }
        fn apply_updates(&mut self, _updates: &[Vec<u8>]) -> Result<(), DocError> {
            self.applied += 1;
            Ok(())
        }
        fn export_snapshot(&self) -> Result<Snapshot, DocError> {
            Ok(Snapshot(Vec::new()))
        }
        fn import_snapshot(&mut self, _data: &Snapshot) -> Result<(), DocError> {
            Ok(())
        }
    }

    fn join(room: &mut Room<StubDoc>, client: ClientId, capability: Capability) {
        let (tx, _rx) = mpsc::unbounded_channel();
        room.on_join(client, tx, capability).expect("join");
    }

    /// The fail-closed core (REVIEW.md #4): a `from` absent from the subscriber
    /// set is rejected, and the rejection happens before the doc is touched.
    #[test]
    fn apply_updates_rejects_unknown_sender_without_touching_doc() {
        let mut room = Room::new(StubDoc::default());
        // Someone else holds the room; the sender below never joined.
        join(&mut room, ClientId::new(1), Capability::Write);

        let result = room.apply_updates(ClientId::new(99), &[vec![0u8]]);

        assert!(matches!(result, Err(UpdateError::Unauthorized)));
        assert_eq!(
            room.doc().applied,
            0,
            "a rejected write must not reach the doc"
        );
    }

    /// Regression guard for the original behavior the fail-closed rewrite must
    /// preserve: a known Read (player) subscriber cannot write.
    #[test]
    fn apply_updates_rejects_read_subscriber() {
        let mut room = Room::new(StubDoc::default());
        let client = ClientId::new(1);
        join(&mut room, client, Capability::Read);

        let result = room.apply_updates(client, &[vec![0u8]]);

        assert!(matches!(result, Err(UpdateError::Unauthorized)));
        assert_eq!(room.doc().applied, 0);
    }

    #[test]
    fn apply_updates_accepts_write_subscriber() {
        let mut room = Room::new(StubDoc::default());
        let client = ClientId::new(1);
        join(&mut room, client, Capability::Write);

        let result = room.apply_updates(client, &[vec![0u8]]);

        assert!(result.is_ok());
        assert_eq!(room.doc().applied, 1);
    }
}
