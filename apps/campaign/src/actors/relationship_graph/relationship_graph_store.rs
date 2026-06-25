//! `RelationshipStore`: the in-memory relationship graph (a `petgraph`
//! multigraph) plus the DB-row -> domain conversion that feeds it. Held by the
//! `RelationshipGraph` actor, which is its only mutator; the store itself is a
//! dumb container with no I/O.

use std::collections::HashMap;

use familiar_systems_campaign_shared::id::{PageId, RelationshipId, SessionId};
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableUnGraph};

use crate::domain::relationship::{Knowledge, Origin, Relationship};
use crate::entities::columns::SessionIdCol;
use crate::entities::relationships;

/// The relationships held in memory: an undirected multigraph (two pages can carry
/// several concurrent edges, plus invalidated history), keyed for the two access
/// patterns the actor needs - all edges touching a page, and one edge by id.
/// Holds **all** rows (live + invalidated): the GM curation view shows superseded +
/// retconned, so liveness is the edge weight's `superseded`/`retcon` stamps, not its
/// presence. `StableUnGraph` keeps edge indices valid across `Delete` removals.
pub(super) struct RelationshipStore {
    graph: StableUnGraph<PageId, Relationship>,
    nodes: HashMap<PageId, NodeIndex>,
    edges_by_id: HashMap<RelationshipId, EdgeIndex>,
}

impl RelationshipStore {
    pub(super) fn new() -> Self {
        Self {
            graph: StableUnGraph::default(),
            nodes: HashMap::new(),
            edges_by_id: HashMap::new(),
        }
    }

    /// The node for a page, creating it on first sight. A page becomes a node only
    /// when an edge first touches it; isolated pages are not represented.
    fn node(&mut self, page: &PageId) -> NodeIndex {
        match self.nodes.get(page) {
            Some(&idx) => idx,
            None => {
                let idx = self.graph.add_node(page.clone());
                self.nodes.insert(page.clone(), idx);
                idx
            }
        }
    }

    pub(super) fn insert(&mut self, rel: Relationship) {
        let a = self.node(&rel.page_a);
        let b = self.node(&rel.page_b);
        let id = rel.id.clone();
        let edge = self.graph.add_edge(a, b, rel);
        self.edges_by_id.insert(id, edge);
    }

    /// Replace an edge's weight in place (its endpoints never change - an op either
    /// mutates fields or invalidates, never moves a relationship between pages).
    fn replace(&mut self, rel: Relationship) {
        if let Some(&edge) = self.edges_by_id.get(&rel.id)
            && let Some(weight) = self.graph.edge_weight_mut(edge)
        {
            *weight = rel;
        }
    }

    /// Insert a relationship, or replace it in place if its id is already present.
    /// The uniform way to reflect a committed write outcome, regardless of which op
    /// produced it (a fresh `Create` inserts; a `SetStamp` of an existing row
    /// replaces).
    pub(super) fn upsert(&mut self, rel: Relationship) {
        if self.edges_by_id.contains_key(&rel.id) {
            self.replace(rel);
        } else {
            self.insert(rel);
        }
    }

    pub(super) fn remove(&mut self, rel_id: &RelationshipId) {
        if let Some(edge) = self.edges_by_id.remove(rel_id) {
            self.graph.remove_edge(edge);
        }
    }

    pub(super) fn get(&self, rel_id: &RelationshipId) -> Option<&Relationship> {
        self.graph.edge_weight(*self.edges_by_id.get(rel_id)?)
    }

    pub(super) fn edges_touching(&self, page: &PageId) -> Vec<&Relationship> {
        match self.nodes.get(page) {
            None => Vec::new(),
            Some(&node) => self.graph.edges(node).map(|e| e.weight()).collect(),
        }
    }

    pub(super) fn all_edges(&self) -> impl Iterator<Item = &Relationship> {
        self.edges_by_id
            .values()
            .filter_map(|&e| self.graph.edge_weight(e))
    }
}

// ---------------------------------------------------------------------------
// Model <-> domain conversion (touches the `*Col` boundary, so it lives here)
// ---------------------------------------------------------------------------

pub(super) fn relationship_from_model(m: relationships::Model) -> Relationship {
    let knowledge = Knowledge::from_columns(m.is_secret, m.reveal_session_id.map(SessionId::from))
        .expect(
            "CHECK (is_secret OR reveal_session_id IS NULL) guarantees a public row \
         carries no reveal",
        );
    Relationship {
        id: m.id.into(),
        page_a: m.page_a.into(),
        page_b: m.page_b.into(),
        predicate_a_to_b: m.predicate_a_to_b,
        predicate_b_to_a: m.predicate_b_to_a,
        origin: origin_from(m.origin_session_id),
        superseded: m.superseded_session_id.map(SessionId::from),
        retcon: m.retcon_session_id.map(SessionId::from),
        knowledge,
        created_at: m.created_at,
    }
}

/// Reconstitute the factuality origin from its nullable session FK: `None` = `Prior`
/// (true before the campaign began).
fn origin_from(session: Option<SessionIdCol>) -> Origin {
    match session {
        Some(sid) => Origin::Session(sid.into()),
        None => Origin::Prior,
    }
}

/// The far endpoint's name. The FK/cascade invariant ("a relationship's endpoints
/// reference live pages") makes a miss impossible; if it is ever broken we surface a
/// loud `Err` (-> 500) rather than substitute an empty name or panic the actor.
pub(super) fn resolve_name(
    names: &HashMap<PageId, String>,
    id: &PageId,
) -> Result<String, sea_orm::DbErr> {
    names.get(id).cloned().ok_or_else(|| {
        sea_orm::DbErr::Custom(format!(
            "relationship endpoint {} missing name (FK ON DELETE CASCADE invariant broken)",
            id.0
        ))
    })
}

/// A referenced session's curated ordinal. `read_session_ordinals` has already
/// verified its map covers every referenced session (erroring otherwise), so a miss
/// here is unreachable; we still surface it as a loud `Err` rather than panic, keeping
/// it consistent with [`resolve_name`].
pub(super) fn resolve_ordinal(
    ordinals: &HashMap<SessionId, i64>,
    sid: &SessionId,
) -> Result<i64, sea_orm::DbErr> {
    ordinals.get(sid).copied().ok_or_else(|| {
        sea_orm::DbErr::Custom(format!(
            "referenced session {} missing ordinal (FK invariant broken)",
            sid.0
        ))
    })
}
