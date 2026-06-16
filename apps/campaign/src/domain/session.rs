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
pub fn next_session_ordinal(prev_max: Option<i64>) -> i64 {
    prev_max.map_or(1, |max| max + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_session_is_one() {
        assert_eq!(next_session_ordinal(None), 1);
    }

    #[test]
    fn each_session_is_one_past_the_max() {
        assert_eq!(next_session_ordinal(Some(1)), 2);
        assert_eq!(next_session_ordinal(Some(13)), 14);
        // A lone Session Zero (ordinal 0) still yields 1 next.
        assert_eq!(next_session_ordinal(Some(0)), 1);
    }
}
