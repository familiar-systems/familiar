//! Kameo-side wiring for fragment reassembly.
//!
//! The pure [`BatchAssembler`](super::assembler::BatchAssembler) tracks
//! fragment buffers but does not enforce the protocol's 10-second
//! reassembly timeout — that requires a scheduler. This module provides:
//!
//! - [`FragmentTimeout`] — a self-message an actor sends to itself at a
//!   batch's deadline.
//! - [`schedule_fragment_timeout`] — helper that the
//!   `Message<DocUpdateFragmentHeader>` handler calls after starting a
//!   batch. Spawns a tokio task that sleeps and `tell`s the timeout
//!   message to the actor.
//!
//! Each actor implements `Message<FragmentTimeout>` directly. The body
//! is a few lines and varies per actor (which ack to emit, what to log)
//! so a shared trait would mostly hide a one-off branch behind a layer
//! of indirection.
//!
//! Stale-fire is benign: when a batch completes before its timeout
//! message arrives, the message still fires; `drop_batch` returns
//! `false`; the handler no-ops. No cancellation bookkeeping needed.

use std::time::Duration;

use familiar_systems_campaign_shared::id::ClientId;
use kameo::actor::ActorRef;
use kameo::message::Message;
use kameo::prelude::Actor;
use loro_protocol::BatchId;

/// Self-message scheduled by [`schedule_fragment_timeout`] at a batch's
/// reassembly deadline. Each actor that owns a `BatchAssembler`
/// implements `Message<FragmentTimeout>` with a body shaped like:
///
/// ```ignore
/// async fn handle(&mut self, msg: FragmentTimeout, _ctx: ...) {
///     if self.assembler.drop_batch(msg.client, msg.batch_id) {
///         self.emit_ack(msg.client, msg.batch_id, AckStatus::FragmentTimeout).await;
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FragmentTimeout {
    pub client: ClientId,
    pub batch_id: BatchId,
}

/// Spawn a tokio task that sleeps for `timeout` and then tells the actor
/// a [`FragmentTimeout`]. The send error is ignored — if the actor is
/// gone by then, there is nothing to do.
///
/// The bound `A: Message<FragmentTimeout, Reply = ()>` is the only
/// compile-time contract: any actor that handles the timeout message
/// can use this helper. There is no enforcement that the actor actually
/// owns a `BatchAssembler`; that's the actor's concern.
pub fn schedule_fragment_timeout<A>(
    actor_ref: ActorRef<A>,
    client: ClientId,
    batch_id: BatchId,
    timeout: Duration,
) where
    A: Actor + Message<FragmentTimeout, Reply = ()>,
{
    tokio::spawn(async move {
        tokio::time::sleep(timeout).await;
        let _ = actor_ref.tell(FragmentTimeout { client, batch_id }).await;
    });
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use kameo::actor::Spawn;
    use kameo::message::{Context, Message};

    use super::super::assembler::{AssembleError, BatchAssembler};
    use super::*;

    fn cid(n: u64) -> ClientId {
        ClientId::new(n)
    }

    fn bid(n: u8) -> BatchId {
        let mut bytes = [0u8; 8];
        bytes[7] = n;
        BatchId(bytes)
    }

    /// A minimal actor that owns an assembler and records timeout fires.
    /// The `Message<FragmentTimeout>` impl below is exactly the shape
    /// each real actor (ThingActor, TocActor, AgentConversationActor)
    /// will write — direct, no trait, no macro.
    struct TestActor {
        assembler: BatchAssembler,
        fired: Arc<Mutex<Vec<(ClientId, BatchId)>>>,
    }

    impl Actor for TestActor {
        type Args = Self;
        type Error = std::convert::Infallible;

        async fn on_start(
            args: Self::Args,
            _: kameo::actor::ActorRef<Self>,
        ) -> Result<Self, Self::Error> {
            Ok(args)
        }
    }

    impl Message<FragmentTimeout> for TestActor {
        type Reply = ();

        async fn handle(
            &mut self,
            msg: FragmentTimeout,
            _ctx: &mut Context<Self, Self::Reply>,
        ) -> Self::Reply {
            if self.assembler.drop_batch(msg.client, msg.batch_id) {
                self.fired.lock().unwrap().push((msg.client, msg.batch_id));
            }
        }
    }

    /// Direct-ask helper to seed assembler state synchronously before
    /// the timeout fires.
    struct StartBatch {
        client: ClientId,
        batch_id: BatchId,
        fragment_count: u64,
        total_size: u64,
    }

    impl Message<StartBatch> for TestActor {
        type Reply = Result<(), AssembleError>;

        async fn handle(
            &mut self,
            msg: StartBatch,
            _ctx: &mut Context<Self, Self::Reply>,
        ) -> Self::Reply {
            self.assembler
                .start(msg.client, msg.batch_id, msg.fragment_count, msg.total_size)
        }
    }

    #[tokio::test]
    async fn timeout_fires_for_in_flight_batch() {
        let fired = Arc::new(Mutex::new(Vec::new()));
        let actor = TestActor {
            assembler: BatchAssembler::new(),
            fired: fired.clone(),
        };
        let actor_ref = TestActor::spawn(actor);

        actor_ref
            .ask(StartBatch {
                client: cid(1),
                batch_id: bid(1),
                fragment_count: 2,
                total_size: 6,
            })
            .await
            .unwrap();

        schedule_fragment_timeout(actor_ref.clone(), cid(1), bid(1), Duration::from_millis(50));

        tokio::time::sleep(Duration::from_millis(150)).await;

        let fired = fired.lock().unwrap();
        assert_eq!(*fired, vec![(cid(1), bid(1))]);
    }

    #[tokio::test]
    async fn timeout_no_ops_for_absent_batch() {
        let fired = Arc::new(Mutex::new(Vec::new()));
        let actor = TestActor {
            assembler: BatchAssembler::new(),
            fired: fired.clone(),
        };
        let actor_ref = TestActor::spawn(actor);

        // Schedule a timeout for a batch that was never started — same
        // shape as the "completed before timeout fired" case from the
        // assembler's perspective (drop_batch returns false either way).
        schedule_fragment_timeout(actor_ref.clone(), cid(1), bid(1), Duration::from_millis(20));

        tokio::time::sleep(Duration::from_millis(80)).await;

        let fired = fired.lock().unwrap();
        assert!(fired.is_empty(), "stale timeout should no-op");
    }
}
