//! Pure relationship algebra: the undirected in-memory model the
//! `RelationshipGraph` actor traverses, plus the three pure rules the actor pulls
//! out of its otherwise-effectful operations - `canonicalize` (the one storage
//! invariant), `orient` (undirected edge -> per-page view), and
//! `known_predicate_pairs` (the vocabulary). No framework deps (no petgraph, no
//! sea-orm); the actor supplies the I/O. Mirrors `domain/session.rs`.
//!
//! These types are pure Rust with no TS surface: the client only ever sees the
//! oriented `RelationshipView`, working in session ordinals, never raw
//! `SessionId`s. The at-rest <-> domain conversion lives with the actor (it
//! touches the `*Col` boundary); this module stays connection-free.
//!
//! See `docs/plans/2026-04-10-entity-relationship-temporal-model.md`.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use familiar_systems_campaign_shared::id::{PageId, RelationshipId, SessionId};
use familiar_systems_campaign_shared::relationship::{
    InvalidationReason, RelatedPage, RelationshipView, ViewInvalidation, ViewSessionOrdinal,
    ViewSessionPoint, Visibility,
};

// ---------------------------------------------------------------------------
// The undirected in-memory model
// ---------------------------------------------------------------------------

/// One relationship edge, in memory: undirected, page-to-page, a predicate at
/// each end plus temporal provenance. The petgraph holds these as edge weights.
///
/// Stored canonically (`page_a` is the lexicographically smaller `PageId`, the
/// predicate pair assigned to match). [`canonicalize`] is the only constructor of
/// the pair, so the invariant holds by construction - a reversed duplicate is
/// impossible. Predicates are immutable: evolution births a new row (supersede),
/// it never edits these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relationship {
    pub id: RelationshipId,
    pub page_a: PageId,
    pub page_b: PageId,
    pub predicate_a_to_b: String,
    pub predicate_b_to_a: String,
    pub visibility: Visibility,
    pub origin: Origin,
    pub created_at: DateTime<Utc>,
    /// `None` while live; `Some` once ended / superseded / retconned.
    pub invalidation: Option<Invalidation>,
}

/// A point in knowledge time: a fact becomes (or ceases to be) true either before
/// the campaign began or in the context of a session. The nullable session FK at
/// rest reconstitutes to this sum at the `*Col`/domain boundary - `None` ->
/// `Prior`. A sum, not `Option<SessionId>`, so `Prior` is a first-class value, not
/// a missing field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    Prior,
    Session(SessionId),
}

impl Origin {
    /// The session FK this point persists as: `Prior` -> `None`.
    pub fn session_id(&self) -> Option<SessionId> {
        match self {
            Origin::Prior => None,
            Origin::Session(s) => Some(s.clone()),
        }
    }
}

/// How and when a relationship stopped being live. `by` reuses [`Origin`] because
/// the at-rest "ended before the campaign began" case (`invalidated_by` NULL) is
/// symmetric with a `Prior` origin. No v1 op produces it (the end/supersede
/// session pickers run origin->current, never "Prior"), but the model must be able
/// to *represent* it to load such a row faithfully.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invalidation {
    pub reason: InvalidationReason,
    pub by: Origin,
    pub at: DateTime<Utc>,
}

/// An invalidation supplied at *creation*, so a relationship can be born already
/// finalized - a retrofit or correction ("X was captain of Y, S3 to S6", entered
/// now; or an AI-proposed historical fact). `at` is not here: like `created_at`, it
/// is stamped at the write edge. A finalized row sits outside the live set, so it
/// never trips the partial unique index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ending {
    pub reason: InvalidationReason,
    pub by: Origin,
}

/// A relationship to persist: canonical endpoints + predicates (built via
/// [`canonicalize`]), its `visibility`, its `origin`, and an optional `ending`
/// (`None` = born live). The owning actor builds this; the writer stamps the id and
/// timestamps. Connection-free, like `NewPage` - the persistence layer maps it to
/// columns (resolving the `Origin` sums to nullable session FKs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRelationship {
    pub page_a: PageId,
    pub page_b: PageId,
    pub predicate_a_to_b: String,
    pub predicate_b_to_a: String,
    pub visibility: Visibility,
    pub origin: Origin,
    pub ending: Option<Ending>,
}

// ---------------------------------------------------------------------------
// Kernel: canonicalize (the one storage invariant)
// ---------------------------------------------------------------------------

/// A relationship's endpoints + predicates in storage form: `page_a` is the
/// lexicographically smaller `PageId`, with the predicate pair assigned to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalEdge {
    pub page_a: PageId,
    pub page_b: PageId,
    pub predicate_a_to_b: String,
    pub predicate_b_to_a: String,
}

impl CanonicalEdge {
    /// Promote a canonicalized edge into a full creation spec by attaching its
    /// lifecycle (visibility, origin, optional born-finalized ending).
    pub fn into_new(
        self,
        visibility: Visibility,
        origin: Origin,
        ending: Option<Ending>,
    ) -> NewRelationship {
        NewRelationship {
            page_a: self.page_a,
            page_b: self.page_b,
            predicate_a_to_b: self.predicate_a_to_b,
            predicate_b_to_a: self.predicate_b_to_a,
            visibility,
            origin,
            ending,
        }
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum EdgeError {
    #[error("a relationship cannot connect a page to itself")]
    SelfEdge,
    #[error("relationship predicates cannot be empty")]
    EmptyPredicate,
}

/// Canonicalize a subject-oriented relationship into storage form.
///
/// `subject`/`other` are the endpoints as the caller named them (the GM is on
/// `subject`'s page); `forward` reads subject->other, `reverse` reads
/// other->subject. The endpoints are ordered by `PageId` and the predicate pair
/// swapped to match, so the stored `(page_a, predicate_a_to_b)` is always keyed on
/// the smaller id. Rejects a self-edge and empty/whitespace predicates - the
/// invariants the entity rustdoc says "the owning actor enforces before any write".
///
/// Ordering is on `PageId` *identity* (immutable: it is the PK / URL / FK target),
/// never on predicate *content* (immutable only by policy). ULIDs are
/// lexicographically sortable, so comparing the inner `Ulid` matches the order of
/// the Crockford-base32 TEXT the column stores - the same order the partial unique
/// index keys on.
pub fn canonicalize(
    subject: PageId,
    other: PageId,
    forward: String,
    reverse: String,
) -> Result<CanonicalEdge, EdgeError> {
    if subject == other {
        return Err(EdgeError::SelfEdge);
    }
    if forward.trim().is_empty() || reverse.trim().is_empty() {
        return Err(EdgeError::EmptyPredicate);
    }

    if subject.0 < other.0 {
        Ok(CanonicalEdge {
            page_a: subject,
            page_b: other,
            predicate_a_to_b: forward,
            predicate_b_to_a: reverse,
        })
    } else {
        Ok(CanonicalEdge {
            page_a: other,
            page_b: subject,
            predicate_a_to_b: reverse,
            predicate_b_to_a: forward,
        })
    }
}

// ---------------------------------------------------------------------------
// Kernel: orient (undirected edge -> per-page view)
// ---------------------------------------------------------------------------

/// Orient an undirected relationship into the per-page read view. `viewed` is the
/// page whose widget is rendering; `predicate` reads forward from it, `other` is
/// the far endpoint. Pure: the caller injects `name_of` (page id -> display name)
/// and `ordinal_of` (session id -> curated ordinal), each total over the ids this
/// edge references (the actor builds them from batch reads). `viewed` is assumed to
/// be one endpoint (the actor only orients edges touching it); the `else` arm reads
/// it as `page_b`.
pub fn orient(
    rel: &Relationship,
    viewed: &PageId,
    name_of: impl Fn(&PageId) -> String,
    ordinal_of: impl Fn(&SessionId) -> i64,
) -> RelationshipView {
    let (other_id, predicate, predicate_reverse) = if viewed == &rel.page_a {
        (&rel.page_b, &rel.predicate_a_to_b, &rel.predicate_b_to_a)
    } else {
        (&rel.page_a, &rel.predicate_b_to_a, &rel.predicate_a_to_b)
    };

    RelationshipView {
        id: rel.id.clone(),
        other: RelatedPage {
            id: other_id.clone(),
            name: name_of(other_id),
        },
        predicate: predicate.clone(),
        predicate_reverse: predicate_reverse.clone(),
        visibility: rel.visibility,
        origin: view_point(&rel.origin, &ordinal_of),
        invalidation: rel.invalidation.as_ref().map(|inv| match inv.reason {
            // The reason is the view's discriminant: a superseded end carries when
            // it ended (a session point, possibly `Prior`); a retcon is timeless and
            // carries nothing.
            InvalidationReason::Superseded => ViewInvalidation::Superseded {
                ended: view_point(&inv.by, &ordinal_of),
            },
            InvalidationReason::Retconned => ViewInvalidation::Retconned,
        }),
    }
}

/// A knowledge-time point (an origin, or a superseded end) in the viewer's terms.
fn view_point(point: &Origin, ordinal_of: impl Fn(&SessionId) -> i64) -> ViewSessionPoint {
    match point {
        Origin::Prior => ViewSessionPoint::Prior,
        Origin::Session(s) => ViewSessionPoint::Session(ViewSessionOrdinal {
            ordinal: ordinal_of(s),
        }),
    }
}

// ---------------------------------------------------------------------------
// Kernel: known_predicate_pairs (the vocabulary)
// ---------------------------------------------------------------------------

/// One known predicate pair and how widely it's used. `forward`/`reverse` is a
/// representative orientation; `count` is the number of edges using the pair in
/// either slot order. Powers the create modal's predicate typeahead + reverse
/// autofill ("reverse of X" = the other label in X's bucket).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredicatePair {
    pub forward: String,
    pub reverse: String,
    pub count: usize,
}

/// Harvest the campaign's predicate vocabulary from the in-memory edges (live +
/// historical).
///
/// Each edge contributes its `(predicate_a_to_b, predicate_b_to_a)` pair, clustered
/// under a canonical *unordered* key (the two labels sorted). Canonicalization
/// orders by page identity, not predicate content, so the same concept can land in
/// either slot across edges; the unordered key clusters them as one. The
/// representative `forward`/`reverse` is the first-seen orientation; `count` is the
/// bucket size. Return order is unspecified (the caller ranks by `count`).
pub fn known_predicate_pairs<'a>(
    edges: impl Iterator<Item = &'a Relationship>,
) -> Vec<PredicatePair> {
    // key = (smaller label, larger label); value = (representative fwd, rev, count).
    let mut buckets: HashMap<(String, String), (String, String, usize)> = HashMap::new();
    for rel in edges {
        let fwd = &rel.predicate_a_to_b;
        let rev = &rel.predicate_b_to_a;
        let key = if fwd <= rev {
            (fwd.clone(), rev.clone())
        } else {
            (rev.clone(), fwd.clone())
        };
        buckets
            .entry(key)
            .and_modify(|(_, _, c)| *c += 1)
            .or_insert_with(|| (fwd.clone(), rev.clone(), 1));
    }
    buckets
        .into_values()
        .map(|(forward, reverse, count)| PredicatePair {
            forward,
            reverse,
            count,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two distinct pages, returned smaller-first by `PageId` order (the canonical
    /// `page_a`/`page_b` order, which matches the stored TEXT order).
    fn two_pages() -> (PageId, PageId) {
        let a = PageId::generate();
        let b = PageId::generate();
        if a.0 < b.0 { (a, b) } else { (b, a) }
    }

    fn rel(
        page_a: PageId,
        page_b: PageId,
        pred_ab: &str,
        pred_ba: &str,
        origin: Origin,
        invalidation: Option<Invalidation>,
    ) -> Relationship {
        Relationship {
            id: RelationshipId::generate(),
            page_a,
            page_b,
            predicate_a_to_b: pred_ab.into(),
            predicate_b_to_a: pred_ba.into(),
            visibility: Visibility::Players,
            origin,
            created_at: Utc::now(),
            invalidation,
        }
    }

    #[test]
    fn canonicalize_keeps_order_when_subject_is_smaller() {
        let (small, big) = two_pages();
        let edge = canonicalize(
            small.clone(),
            big.clone(),
            "is captain of".into(),
            "is captained by".into(),
        )
        .unwrap();
        assert_eq!(edge.page_a, small);
        assert_eq!(edge.page_b, big);
        assert_eq!(edge.predicate_a_to_b, "is captain of");
        assert_eq!(edge.predicate_b_to_a, "is captained by");
    }

    #[test]
    fn canonicalize_swaps_when_subject_is_larger() {
        let (small, big) = two_pages();
        // Subject is the larger page: endpoints flip and the predicate pair swaps so
        // `(page_a, predicate_a_to_b)` stays keyed on the smaller id.
        let edge = canonicalize(
            big.clone(),
            small.clone(),
            "is captain of".into(),
            "is captained by".into(),
        )
        .unwrap();
        assert_eq!(edge.page_a, small);
        assert_eq!(edge.page_b, big);
        assert_eq!(
            edge.predicate_a_to_b, "is captained by",
            "small->big = the reverse"
        );
        assert_eq!(
            edge.predicate_b_to_a, "is captain of",
            "big->small = the forward"
        );
    }

    #[test]
    fn canonicalize_rejects_self_edge() {
        let p = PageId::generate();
        assert_eq!(
            canonicalize(p.clone(), p, "knows".into(), "knows".into()),
            Err(EdgeError::SelfEdge)
        );
    }

    #[test]
    fn canonicalize_rejects_empty_or_whitespace_predicate() {
        let (a, b) = two_pages();
        assert_eq!(
            canonicalize(a.clone(), b.clone(), "".into(), "x".into()),
            Err(EdgeError::EmptyPredicate)
        );
        assert_eq!(
            canonicalize(a, b, "x".into(), "   ".into()),
            Err(EdgeError::EmptyPredicate)
        );
    }

    #[test]
    fn orient_reads_forward_from_each_endpoint() {
        let (a, b) = two_pages();
        let r = rel(
            a.clone(),
            b.clone(),
            "is a resident of",
            "is the home of",
            Origin::Prior,
            None,
        );
        let names = |id: &PageId| {
            if id == &a {
                "John".into()
            } else {
                "Townsville".into()
            }
        };
        let ords = |_: &SessionId| 0;

        let from_a = orient(&r, &a, names, ords);
        assert_eq!(from_a.other.name, "Townsville");
        assert_eq!(from_a.predicate, "is a resident of");
        assert_eq!(from_a.predicate_reverse, "is the home of");

        let from_b = orient(&r, &b, names, ords);
        assert_eq!(from_b.other.name, "John");
        assert_eq!(from_b.predicate, "is the home of");
        assert_eq!(from_b.predicate_reverse, "is a resident of");
    }

    #[test]
    fn orient_maps_origin_and_invalidation_to_ordinals() {
        let (a, b) = two_pages();
        let origin = SessionId::generate();
        let ender = SessionId::generate();
        let r = rel(
            a.clone(),
            b.clone(),
            "is captain of",
            "is captained by",
            Origin::Session(origin.clone()),
            Some(Invalidation {
                reason: InvalidationReason::Superseded,
                by: Origin::Session(ender.clone()),
                at: Utc::now(),
            }),
        );
        let names = |_: &PageId| "Guild".to_string();
        let ords = |s: &SessionId| if s == &origin { 6 } else { 12 };

        let view = orient(&r, &a, names, ords);
        match view.origin {
            ViewSessionPoint::Session(s) => assert_eq!(s.ordinal, 6),
            other => panic!("expected Session origin, got {other:?}"),
        }
        let inv = view
            .invalidation
            .expect("invalidated row carries a view invalidation");
        match inv {
            ViewInvalidation::Superseded {
                ended: ViewSessionPoint::Session(s),
            } => assert_eq!(s.ordinal, 12, "ended at the invalidating session's ordinal"),
            other => panic!("expected Superseded-at-session, got {other:?}"),
        }
    }

    #[test]
    fn orient_prior_origin_and_live_row() {
        let (a, b) = two_pages();
        let r = rel(a.clone(), b, "knows", "knows", Origin::Prior, None);
        let view = orient(&r, &a, |_| "X".to_string(), |_| 0);
        assert!(matches!(view.origin, ViewSessionPoint::Prior));
        assert!(
            view.invalidation.is_none(),
            "a live row has no invalidation"
        );
    }

    #[test]
    fn known_predicate_pairs_clusters_regardless_of_slot_order() {
        let (a, b) = two_pages();
        // Same concept, opposite slot order across two edges (canonicalization keys
        // on page identity, so this is normal), plus a symmetric pair.
        let edges = [
            rel(
                a.clone(),
                b.clone(),
                "is a resident of",
                "is the home of",
                Origin::Prior,
                None,
            ),
            rel(
                a.clone(),
                b.clone(),
                "is the home of",
                "is a resident of",
                Origin::Prior,
                None,
            ),
            rel(
                a.clone(),
                b.clone(),
                "is allied with",
                "is allied with",
                Origin::Prior,
                None,
            ),
        ];

        let mut pairs = known_predicate_pairs(edges.iter());
        pairs.sort_by_key(|p| std::cmp::Reverse(p.count));

        assert_eq!(
            pairs.len(),
            2,
            "the resident/home pair clusters into one bucket"
        );
        assert_eq!(pairs[0].count, 2, "both orderings counted together");
        let labels = [pairs[0].forward.as_str(), pairs[0].reverse.as_str()];
        assert!(labels.contains(&"is a resident of") && labels.contains(&"is the home of"));
        assert_eq!(pairs[1].count, 1);
        assert_eq!(pairs[1].forward, "is allied with");
    }
}
