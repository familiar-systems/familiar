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
        if let Some(sub) = self.subscribers.get(&from) {
            if sub.capability == Capability::Read {
                return Err(UpdateError::Unauthorized);
            }
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
