//! Shared persistence state machine for the CRDT room actors.
//!
//! Both [`TocActor`](super::toc::TocActor) and
//! [`PageActor`](super::page::PageActor) debounce client edits and flush a
//! full snapshot of their in-memory Loro doc to the
//! [`DatabaseWriteActor`](super::database_writer::DatabaseWriteActor). They used
//! to encode that lifecycle as a `dirty: bool` plus a
//! `persist_timer: Option<JoinHandle>`, two independent fields whose product
//! admits illegal combinations:
//!
//! - `(dirty, no timer)`: dirty but nothing will ever flush it. This was a real
//!   bug: the ToC marked itself dirty on a client edit but forgot to arm the
//!   timer, so structural edits only reached disk on full-campaign drain.
//! - `(clean, timer)`: a flush pending with nothing to write.
//!
//! [`Persist`] collapses those two fields into one sum type. You cannot be dirty
//! without holding the timer that will flush you, so "dirty but unscheduled" is
//! unrepresentable. The duplication of this machine across the two actors is
//! also what let them drift in the first place; sharing it is the fix.
//!
//! ## Durability: the flush is awaited, not fire-and-forget
//!
//! The flush goes out as an `ask` (await the commit), not a `tell`
//! (fire-and-forget). The room actor clears `dirty` only after the write is
//! durable. A `tell` cleared `dirty` the instant the message was *enqueued*, so
//! a write that never committed left the actor falsely believing it was clean.
//! `ask` costs a brief mailbox stall per flush (a single-digit-ms SQLite
//! commit), which the debounce window already hides.
//!
//! ## Retry, but no eviction gate
//!
//! A failed flush keeps the actor dirty and re-arms the timer: the data is still
//! in the Loro doc and the write is an idempotent full-snapshot replace, so
//! retrying is safe. Persistence health does **not** gate room eviction. By the
//! time an idle timer fires (tens of seconds), a still-dirty actor has retried
//! many times; if it is still failing, persistence is persistently broken and
//! pinning the actor resident would only march the process toward OOM, turning a
//! persistence outage into a total one. So a dirty actor evicts anyway, attempts
//! a final flush in `on_stop`, and logs the loss.

use std::time::Duration;

use kameo::actor::ActorRef;
use kameo::error::SendError;
use kameo::prelude::{Actor, Message};

/// Debounce-fired message both room actors handle to flush their dirty doc.
///
/// Always sent via `tell` from the debounce timer, so its reply is `()`.
#[derive(Debug, Clone, Copy)]
pub struct PersistNow;

/// Consecutive failed flushes past which the retry log escalates from `warn` to
/// `error`. Below it a blip is expected noise; at or above it persistence is
/// plausibly broken and deserves a louder signal.
const DEGRADED_AFTER: u32 = 2;

/// Why a flush could not be made durable. Either the write handler itself
/// errored (a real DB failure), or the message never reached the writer (it
/// died or is overloaded). Both keep the actor dirty; the distinction is only
/// for logging.
#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("write handler failed: {0}")]
    Db(sea_orm::DbErr),
    #[error("database writer unreachable: {0}")]
    Unreachable(&'static str),
}

// Collapse a writer `ask` failure into a `PersistError`. The handler's own
// error (`HandlerError`) carries the `DbErr`; every other variant means the
// message never got a durable reply. Matching the variants avoids requiring the
// message type to be `Debug` just to format the error.
impl<M> From<SendError<M, sea_orm::DbErr>> for PersistError {
    fn from(err: SendError<M, sea_orm::DbErr>) -> Self {
        match err {
            SendError::HandlerError(db) => PersistError::Db(db),
            SendError::ActorNotRunning(_) => PersistError::Unreachable("writer not running"),
            SendError::ActorStopped => PersistError::Unreachable("writer stopped"),
            SendError::MailboxFull(_) => PersistError::Unreachable("writer mailbox full"),
            SendError::Timeout(_) => PersistError::Unreachable("writer timeout"),
        }
    }
}

/// Persistence state for a room actor's CRDT doc.
///
/// `Clean`: the in-memory doc matches what is durable in SQLite.
/// `Pending`: the doc has edits not yet durably written, and a debounce timer is
/// armed that will fire [`PersistNow`]. The timer handle is inseparable from
/// dirtiness, which is the whole point: there is no way to be dirty without a
/// scheduled flush.
#[derive(Debug, Default)]
pub enum Persist {
    #[default]
    Clean,
    Pending {
        /// The armed debounce task. Aborted on transition out of `Pending`.
        timer: tokio::task::JoinHandle<()>,
        /// Consecutive failed flush attempts. Zero for the first debounce after
        /// a clean -> dirty edit; bumped each time a flush fails and reschedules.
        /// A fresh edit while already `Pending` preserves it (so an edit during
        /// an outage does not reset the failure count).
        attempts: u32,
    },
}

impl Persist {
    pub fn new() -> Self {
        Persist::Clean
    }

    pub fn is_dirty(&self) -> bool {
        matches!(self, Persist::Pending { .. })
    }

    fn attempts(&self) -> u32 {
        match self {
            Persist::Clean => 0,
            Persist::Pending { attempts, .. } => *attempts,
        }
    }

    /// Mark dirty and (re)arm the debounce timer, preserving the current attempt
    /// count. Called from message handlers when the doc advances. Idempotent
    /// re-arming: a burst of edits collapses to one pending flush.
    pub fn schedule<A>(&mut self, owner: &ActorRef<A>, debounce: Duration)
    where
        A: Actor + Message<PersistNow>,
    {
        let attempts = self.attempts();
        self.arm(owner, debounce, attempts);
    }

    /// Resolve a flush attempt. On success, go `Clean`. On failure, stay dirty
    /// and re-arm with a bumped attempt count so the timer retries; the data is
    /// still in the doc, so this loses nothing. Logs at `warn`, escalating to
    /// `error` once failures persist.
    pub fn after_flush<A>(
        &mut self,
        result: Result<(), PersistError>,
        owner: &ActorRef<A>,
        debounce: Duration,
    ) where
        A: Actor + Message<PersistNow>,
    {
        match result {
            Ok(()) => self.mark_clean(),
            Err(err) => {
                let attempts = self.attempts().saturating_add(1);
                if attempts >= DEGRADED_AFTER {
                    tracing::error!(attempts, error = %err, "persist repeatedly failing; data held in memory");
                } else {
                    tracing::warn!(attempts, error = %err, "persist failed, will retry");
                }
                self.arm(owner, debounce, attempts);
            }
        }
    }

    /// Transition to `Clean` after a durable write, cancelling any armed timer.
    pub fn mark_clean(&mut self) {
        if let Persist::Pending { timer, .. } = self {
            timer.abort();
        }
        *self = Persist::Clean;
    }

    /// (Re)arm the debounce timer at the given attempt count, aborting any prior
    /// timer first so only one is ever live.
    fn arm<A>(&mut self, owner: &ActorRef<A>, debounce: Duration, attempts: u32)
    where
        A: Actor + Message<PersistNow>,
    {
        if let Persist::Pending { timer, .. } = self {
            timer.abort();
        }
        let owner = owner.clone();
        let timer = tokio::spawn(async move {
            tokio::time::sleep(debounce).await;
            let _ = owner.tell(PersistNow).send().await;
        });
        *self = Persist::Pending { timer, attempts };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kameo::actor::{ActorRef, Spawn};
    use kameo::message::{Context, Message};
    use kameo::prelude::Actor;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Minimal actor that just counts how many `PersistNow` messages the
    /// debounce timer delivered. Lets the state machine be driven in isolation
    /// from the real room actors, with no DB.
    struct Probe {
        fires: Arc<AtomicU32>,
    }

    impl Actor for Probe {
        type Args = Arc<AtomicU32>;
        type Error = std::convert::Infallible;
        async fn on_start(args: Self::Args, _r: ActorRef<Self>) -> Result<Self, Self::Error> {
            Ok(Probe { fires: args })
        }
    }

    impl Message<PersistNow> for Probe {
        type Reply = ();
        async fn handle(
            &mut self,
            _: PersistNow,
            _ctx: &mut Context<Self, Self::Reply>,
        ) -> Self::Reply {
            self.fires.fetch_add(1, Ordering::SeqCst);
        }
    }

    const DEBOUNCE: Duration = Duration::from_millis(40);

    fn ok() -> Result<(), PersistError> {
        Ok(())
    }
    fn err() -> Result<(), PersistError> {
        Err(PersistError::Unreachable("test"))
    }

    // 4x the debounce: generous enough to absorb scheduler jitter in CI.
    async fn settle() {
        tokio::time::sleep(Duration::from_millis(160)).await;
    }

    #[tokio::test]
    async fn schedule_fires_after_debounce() {
        let fires = Arc::new(AtomicU32::new(0));
        let probe = Probe::spawn(fires.clone());

        let mut p = Persist::new();
        assert!(!p.is_dirty());
        p.schedule(&probe, DEBOUNCE);
        assert!(p.is_dirty(), "scheduling makes it dirty immediately");

        settle().await;
        assert_eq!(
            fires.load(Ordering::SeqCst),
            1,
            "the debounce timer flushed once"
        );
    }

    #[tokio::test]
    async fn repeated_schedule_coalesces_to_one_fire() {
        let fires = Arc::new(AtomicU32::new(0));
        let probe = Probe::spawn(fires.clone());

        let mut p = Persist::new();
        // A burst of edits, each within the debounce window, must collapse to a
        // single flush (each schedule re-arms, aborting the prior timer).
        for _ in 0..5 {
            p.schedule(&probe, DEBOUNCE);
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        settle().await;
        assert_eq!(
            fires.load(Ordering::SeqCst),
            1,
            "a burst collapses to one flush"
        );
    }

    #[tokio::test]
    async fn mark_clean_cancels_pending_timer() {
        let fires = Arc::new(AtomicU32::new(0));
        let probe = Probe::spawn(fires.clone());

        let mut p = Persist::new();
        p.schedule(&probe, DEBOUNCE);
        p.mark_clean();
        assert!(!p.is_dirty());

        settle().await;
        assert_eq!(
            fires.load(Ordering::SeqCst),
            0,
            "a cancelled timer never fires"
        );
    }

    #[tokio::test]
    async fn after_flush_ok_marks_clean() {
        let fires = Arc::new(AtomicU32::new(0));
        let probe = Probe::spawn(fires.clone());

        let mut p = Persist::new();
        p.schedule(&probe, DEBOUNCE);
        p.after_flush(ok(), &probe, DEBOUNCE);
        assert!(!p.is_dirty(), "a durable write clears dirtiness");

        settle().await;
        assert_eq!(fires.load(Ordering::SeqCst), 0, "no retry after success");
    }

    #[tokio::test]
    async fn after_flush_err_stays_dirty_and_reschedules() {
        let fires = Arc::new(AtomicU32::new(0));
        let probe = Probe::spawn(fires.clone());

        let mut p = Persist::new();
        p.schedule(&probe, DEBOUNCE);
        // The flush failed: this is the bug-3 case. Dirtiness must survive, and a
        // retry must be armed so the data isn't stranded only-in-memory.
        p.after_flush(err(), &probe, DEBOUNCE);
        assert!(p.is_dirty(), "a failed flush must not clear dirtiness");

        settle().await;
        assert!(
            fires.load(Ordering::SeqCst) >= 1,
            "a failed flush reschedules a retry"
        );
    }
}
