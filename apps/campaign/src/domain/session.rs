//! The one pure rule in session creation: the next ordinal.
//!
//! Minting a session is otherwise all effect - reading the campaign's current
//! highest ordinal, generating the `SessionId`, the insert - and that effect
//! lives at the write edge in `DbCreateSession`, composed into the page's
//! genesis transaction. This is the single invariant pulled out as a pure,
//! deterministic, unit-testable function: the functional core of an otherwise
//! effectful operation, per the project's Ghosh-style design.

/// The ordinal to assign a freshly minted session: one past the campaign's
/// current highest.
///
/// `prev_max` is the highest existing `sessions.ordinal`, or `None` when no
/// sessions exist yet, in which case the first session is `1`. A "Session Zero"
/// (setup / character creation) is a deliberate GM renumber down to `0`, never
/// the auto-assigned default; the schema's `CHECK (ordinal >= 0)` permits it.
///
/// Ordinals are GM-curated and reorderable, so reorder gaps are irrelevant here:
/// this only needs to hand back a unique, increasing value at birth.
/// The campaign already holds `i64::MAX` sessions, so there is no next ordinal.
/// Unreachable in practice (it would take ~9.2e18 sessions), but returning it as
/// a typed error rather than computing `max + 1` keeps an overflow from panicking
/// in debug or silently wrapping to a negative ordinal in release.
#[derive(Debug, thiserror::Error)]
#[error("session ordinal overflow: campaign already holds i64::MAX sessions")]
pub struct SessionOrdinalOverflow;

pub fn next_session_ordinal(prev_max: Option<i64>) -> Result<i64, SessionOrdinalOverflow> {
    match prev_max {
        None => Ok(1),
        Some(max) => max.checked_add(1).ok_or(SessionOrdinalOverflow),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_session_is_one() {
        assert_eq!(next_session_ordinal(None).unwrap(), 1);
    }

    #[test]
    fn each_session_is_one_past_the_max() {
        assert_eq!(next_session_ordinal(Some(1)).unwrap(), 2);
        assert_eq!(next_session_ordinal(Some(13)).unwrap(), 14);
        // A lone Session Zero (ordinal 0) still yields 1 next.
        assert_eq!(next_session_ordinal(Some(0)).unwrap(), 1);
    }

    #[test]
    fn overflow_at_i64_max_is_a_typed_error() {
        // Unreachable in practice, but it must fail loudly rather than wrap.
        assert!(next_session_ordinal(Some(i64::MAX)).is_err());
    }
}
