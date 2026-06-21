//! `RelationshipGraph`: the server-authoritative, in-memory relationship graph.
//!
//! An eager singleton child of the [`CampaignSupervisor`](crate::actors::supervisor::CampaignSupervisor),
//! mirroring [`TocActor`](crate::actors::toc::TocActor) - but it is **not** a CRDT
//! room. It holds the campaign's relationships in a `petgraph` (nodes = `PageId`,
//! edges = [`Relationship`]) and is the single consistency boundary every mutation
//! flows through: it validates + canonicalizes, decomposes each op into an ordered
//! list of single-statement writes the single-writer [`DatabaseWriteActor`] runs in
//! one transaction (supersede = create + invalidate, atomically), then reflects the
//! committed row(s) into the in-memory graph. The graph therefore never drifts (it
//! only ever reflects committed state), so there is no debounce/`Persist` machine
//! and nothing to flush on stop. On restart it reloads every row from the table (no
//! CRDT snapshot).
//!
//! Reads orient stored undirected edges into the per-page `RelationshipView`,
//! resolving page names + session ordinals fresh from the reader pool (auxiliary
//! projection data, not held in the graph). See
//! `docs/plans/2026-04-10-entity-relationship-temporal-model.md`.

use std::collections::HashMap;

use familiar_systems_app_shared::id::CampaignId;
use familiar_systems_campaign_shared::id::{PageId, RelationshipId, SessionId};
use familiar_systems_campaign_shared::relationship::{
    InvalidationReason, RelationshipView, Visibility,
};
use kameo::actor::ActorRef;
use kameo::error::SendError;
use kameo::message::{Context, Message};
use kameo::prelude::Actor;
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableUnGraph};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::actors::database_writer::{
    ApplyRelationshipWrites, DatabaseWriteActor, RelationshipWrite, RelationshipWriteError,
    RelationshipWriteOutcome,
};
use crate::domain::relationship::{
    EdgeError, Ending, Invalidation, Origin, PredicatePair, Relationship, canonicalize,
    known_predicate_pairs, orient,
};
use crate::entities::columns::{PageIdCol, SessionIdCol};
use crate::entities::{pages, relationships, sessions};

// ---------------------------------------------------------------------------
// In-memory store (petgraph wrapper)
// ---------------------------------------------------------------------------

/// The relationships held in memory: an undirected multigraph (two pages can carry
/// several concurrent edges, plus invalidated history), keyed for the two access
/// patterns the actor needs - all edges touching a page, and one edge by id.
/// Holds **all** rows (live + invalidated): the GM curation view shows superseded +
/// retconned, so liveness is the edge weight's `invalidation.is_some()`, not its
/// presence. `StableUnGraph` keeps edge indices valid across `Delete` removals.
struct RelationshipStore {
    graph: StableUnGraph<PageId, Relationship>,
    nodes: HashMap<PageId, NodeIndex>,
    edges_by_id: HashMap<RelationshipId, EdgeIndex>,
}

impl RelationshipStore {
    fn new() -> Self {
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

    fn insert(&mut self, rel: Relationship) {
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
    /// produced it (a fresh `Create` inserts; an `Invalidate`/`SetVisibility` of an
    /// existing row replaces).
    fn upsert(&mut self, rel: Relationship) {
        if self.edges_by_id.contains_key(&rel.id) {
            self.replace(rel);
        } else {
            self.insert(rel);
        }
    }

    fn remove(&mut self, rel_id: &RelationshipId) {
        if let Some(edge) = self.edges_by_id.remove(rel_id) {
            self.graph.remove_edge(edge);
        }
    }

    fn get(&self, rel_id: &RelationshipId) -> Option<&Relationship> {
        self.graph.edge_weight(*self.edges_by_id.get(rel_id)?)
    }

    fn edges_touching(&self, page: &PageId) -> Vec<&Relationship> {
        match self.nodes.get(page) {
            None => Vec::new(),
            Some(&node) => self.graph.edges(node).map(|e| e.weight()).collect(),
        }
    }

    fn all_edges(&self) -> impl Iterator<Item = &Relationship> {
        self.edges_by_id
            .values()
            .filter_map(|&e| self.graph.edge_weight(e))
    }
}

// ---------------------------------------------------------------------------
// Model <-> domain conversion (touches the `*Col` boundary, so it lives here)
// ---------------------------------------------------------------------------

fn relationship_from_model(m: relationships::Model) -> Relationship {
    let origin = origin_from(m.origin_session_id);
    let invalidation = match m.invalidation_reason {
        None => None,
        Some(reason) => Some(Invalidation {
            reason: reason.into(),
            by: origin_from(m.invalidated_by_session_id),
            at: m.invalidated_at.expect(
                "CHECK ((invalidation_reason IS NULL) = (invalidated_at IS NULL)) guarantees \
                 invalidated_at is set whenever a reason is",
            ),
        }),
    };
    Relationship {
        id: m.id.into(),
        page_a: m.page_a.into(),
        page_b: m.page_b.into(),
        predicate_a_to_b: m.predicate_a_to_b,
        predicate_b_to_a: m.predicate_b_to_a,
        visibility: m.visibility.into(),
        origin,
        created_at: m.created_at,
        invalidation,
    }
}

/// Reconstitute a knowledge-time point from its nullable session FK: `None` =
/// `Prior` (true / ended before the campaign began).
fn origin_from(session: Option<SessionIdCol>) -> Origin {
    match session {
        Some(sid) => Origin::Session(sid.into()),
        None => Origin::Prior,
    }
}

/// The far endpoint's name, with the FK/cascade invariant ("a relationship's
/// endpoints reference live pages") logged loudly if ever broken rather than
/// panicking the read.
fn resolve_name(names: &HashMap<PageId, String>, id: &PageId) -> String {
    names.get(id).cloned().unwrap_or_else(|| {
        tracing::error!(page_id = %id.0, "relationship endpoint missing from pages (FK/cascade invariant broken)");
        String::new()
    })
}

/// A referenced session's curated ordinal. Total: `read_session_ordinals` has
/// already verified its map covers every referenced session (erroring otherwise),
/// so the lookup cannot miss.
fn resolve_ordinal(ordinals: &HashMap<SessionId, i64>, sid: &SessionId) -> i64 {
    *ordinals
        .get(sid)
        .expect("referenced session ordinal present (validated in read_session_ordinals)")
}

// ---------------------------------------------------------------------------
// Actor
// ---------------------------------------------------------------------------

pub struct RelationshipGraph {
    campaign_id: CampaignId,
    /// Retained (unlike `TocActor`) to resolve page names + session ordinals at
    /// view-build and to pre-check page existence on create. These are auxiliary
    /// projection reads, not graph state.
    db_reader: DatabaseConnection,
    db_writer: ActorRef<DatabaseWriteActor>,
    store: RelationshipStore,
}

pub struct RelationshipGraphArgs {
    pub campaign_id: CampaignId,
    pub db_reader: DatabaseConnection,
    pub db_writer: ActorRef<DatabaseWriteActor>,
}

impl Actor for RelationshipGraph {
    type Args = RelationshipGraphArgs;
    type Error = sea_orm::DbErr;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %args.campaign_id.0),
    )]
    async fn on_start(args: Self::Args, _actor_ref: ActorRef<Self>) -> Result<Self, Self::Error> {
        let rows = relationships::Entity::find()
            .all(&args.db_reader)
            .await
            .inspect_err(|e| tracing::error!(error = %e, "failed to load relationships"))?;

        let mut store = RelationshipStore::new();
        for row in rows {
            store.insert(relationship_from_model(row));
        }
        tracing::debug!(
            edges = store.edges_by_id.len(),
            "relationship graph started"
        );

        Ok(Self {
            campaign_id: args.campaign_id,
            db_reader: args.db_reader,
            db_writer: args.db_writer,
            store,
        })
    }
}

impl RelationshipGraph {
    async fn read_page_names(
        &self,
        ids: &[PageId],
    ) -> Result<HashMap<PageId, String>, sea_orm::DbErr> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let cols: Vec<PageIdCol> = ids.iter().cloned().map(PageIdCol::from).collect();
        let rows = pages::Entity::find()
            .filter(pages::Column::Id.is_in(cols))
            .all(&self.db_reader)
            .await?;
        Ok(rows
            .into_iter()
            .map(|p| (PageId::from(p.id), p.name))
            .collect())
    }

    async fn read_session_ordinals(
        &self,
        ids: &[SessionId],
    ) -> Result<HashMap<SessionId, i64>, sea_orm::DbErr> {
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let cols: Vec<SessionIdCol> = ids.iter().cloned().map(SessionIdCol::from).collect();
        let rows = sessions::Entity::find()
            .filter(sessions::Column::Id.is_in(cols))
            .all(&self.db_reader)
            .await?;
        let ordinals: HashMap<SessionId, i64> = rows
            .into_iter()
            .map(|s| (SessionId::from(s.id), s.ordinal))
            .collect();
        // The session FKs are NO ACTION, so every referenced session must still
        // exist; a gap is a broken invariant. Fail loudly rather than emit a
        // sentinel ordinal into the view.
        if let Some(missing) = ids.iter().find(|id| !ordinals.contains_key(id)) {
            return Err(sea_orm::DbErr::Custom(format!(
                "referenced session {} missing ordinal (FK invariant broken)",
                missing.0
            )));
        }
        Ok(ordinals)
    }

    /// One session's curated ordinal, or `None` if no such session exists. Distinct
    /// from [`read_session_ordinals`](Self::read_session_ordinals), which treats a gap
    /// as a broken FK invariant (a 500): here a missing session is *client input* (a
    /// stale id from the create/end body), so the caller maps `None` to a clean 404.
    async fn session_ordinal(&self, s: &SessionId) -> Result<Option<i64>, sea_orm::DbErr> {
        Ok(sessions::Entity::find_by_id(SessionIdCol::from(s.clone()))
            .one(&self.db_reader)
            .await?
            .map(|row| row.ordinal))
    }

    /// The session ids an edge references (origin + invalidation), for batch ordinal
    /// resolution.
    fn referenced_sessions(rel: &Relationship, out: &mut Vec<SessionId>) {
        if let Origin::Session(s) = &rel.origin {
            out.push(s.clone());
        }
        if let Some(inv) = &rel.invalidation
            && let Origin::Session(s) = &inv.by
        {
            out.push(s.clone());
        }
    }

    /// Build the oriented view of one relationship, resolving its auxiliary reads.
    async fn view_for(
        &self,
        rel: &Relationship,
        viewed: &PageId,
    ) -> Result<RelationshipView, sea_orm::DbErr> {
        let other = if &rel.page_a == viewed {
            &rel.page_b
        } else {
            &rel.page_a
        };
        let names = self.read_page_names(std::slice::from_ref(other)).await?;
        let mut session_ids = Vec::new();
        Self::referenced_sessions(rel, &mut session_ids);
        let ordinals = self.read_session_ordinals(&session_ids).await?;
        Ok(orient(
            rel,
            viewed,
            |id| resolve_name(&names, id),
            |sid| resolve_ordinal(&ordinals, sid),
        ))
    }

    async fn ensure_page_exists(&self, page: &PageId) -> Result<(), CreateRelationshipError> {
        let found = pages::Entity::find_by_id(PageIdCol::from(page.clone()))
            .one(&self.db_reader)
            .await?;
        if found.is_none() {
            return Err(CreateRelationshipError::PageNotFound(page.clone()));
        }
        Ok(())
    }

    /// Reflect a committed batch's outcomes into the in-memory graph: upsert each
    /// returned row, remove each deleted id. Uniform across every op, so the actor
    /// never correlates an outcome's position back to the write that produced it.
    fn reflect(&mut self, outcomes: Vec<RelationshipWriteOutcome>) {
        for outcome in outcomes {
            match outcome {
                RelationshipWriteOutcome::Upserted(model) => {
                    self.store.upsert(relationship_from_model(model));
                }
                RelationshipWriteOutcome::Removed(id) => self.store.remove(&id),
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------

/// All relationships touching a page (live + invalidated), oriented to the page -
/// the GM curation view the widget renders.
#[derive(Debug, Clone)]
pub struct RelationshipsForPage {
    pub page_id: PageId,
}

impl Message<RelationshipsForPage> for RelationshipGraph {
    type Reply = Result<Vec<RelationshipView>, sea_orm::DbErr>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, page_id = %msg.page_id.0),
    )]
    async fn handle(
        &mut self,
        msg: RelationshipsForPage,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        // Clone the touching edges so no borrow of the graph is held across the
        // auxiliary reads below.
        let edges: Vec<Relationship> = self
            .store
            .edges_touching(&msg.page_id)
            .into_iter()
            .cloned()
            .collect();
        if edges.is_empty() {
            return Ok(Vec::new());
        }

        // Batch the auxiliary reads: far-endpoint names + referenced session ordinals.
        let mut other_ids = Vec::with_capacity(edges.len());
        let mut session_ids = Vec::new();
        for rel in &edges {
            let other = if rel.page_a == msg.page_id {
                &rel.page_b
            } else {
                &rel.page_a
            };
            other_ids.push(other.clone());
            Self::referenced_sessions(rel, &mut session_ids);
        }

        let names = self.read_page_names(&other_ids).await?;
        let ordinals = self.read_session_ordinals(&session_ids).await?;

        let views = edges
            .iter()
            .map(|rel| {
                orient(
                    rel,
                    &msg.page_id,
                    |id| resolve_name(&names, id),
                    |sid| resolve_ordinal(&ordinals, sid),
                )
            })
            .collect();
        Ok(views)
    }
}

/// Create a relationship from `subject` toward `other`. `predicate_forward` reads
/// subject->other; the actor canonicalizes and returns the view oriented to
/// `subject` (the page the create came from). `ending` is `None` for the v1 create
/// UI; `Some` births the relationship already finalized (a retrofit/correction).
///
/// `supersedes` makes it an atomic *replace*: the named live row is ended in the
/// same transaction, at this create's origin session (so `origin` must be a
/// session). The new row births live, so `ending` is ignored when `supersedes` is
/// set. This is the manual analog of an AI-proposed replacement.
#[derive(Debug, Clone)]
pub struct CreateRelationship {
    pub subject: PageId,
    pub other: PageId,
    pub predicate_forward: String,
    pub predicate_reverse: String,
    pub visibility: Visibility,
    pub origin: Origin,
    pub ending: Option<Ending>,
    pub supersedes: Option<RelationshipId>,
}

#[derive(Debug, thiserror::Error)]
pub enum CreateRelationshipError {
    #[error("a relationship cannot connect a page to itself")]
    SelfEdge,
    #[error("relationship predicates cannot be empty")]
    EmptyPredicate,
    #[error("page not found: {0}")]
    PageNotFound(PageId),
    #[error("a live relationship with this predicate pair already exists")]
    DuplicateLiveFact,
    #[error("the relationship being superseded does not exist")]
    SupersedesNotFound,
    #[error("the relationship being superseded is already invalidated")]
    SupersedesNotLive,
    #[error("a supersede must replace a fact between the same two pages")]
    SupersedesDifferentPair,
    #[error("a supersede must originate at a session, not before the campaign began")]
    PriorOriginCannotSupersede,
    #[error("referenced session not found: {0}")]
    SessionNotFound(SessionId),
    #[error("a supersede cannot take effect before the fact it replaces began")]
    EndBeforeOrigin,
    #[error("relationship write actor unavailable")]
    ActorUnavailable,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

impl From<EdgeError> for CreateRelationshipError {
    fn from(e: EdgeError) -> Self {
        match e {
            EdgeError::SelfEdge => Self::SelfEdge,
            EdgeError::EmptyPredicate => Self::EmptyPredicate,
        }
    }
}

impl Message<CreateRelationship> for RelationshipGraph {
    type Reply = Result<RelationshipView, CreateRelationshipError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        msg: CreateRelationship,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        let subject = msg.subject.clone();
        let edge = canonicalize(
            msg.subject,
            msg.other,
            msg.predicate_forward,
            msg.predicate_reverse,
        )?;

        // Both endpoints must exist. The FK would also reject a dangling page, but a
        // pre-check yields a clean typed error; race-free under the single-writer +
        // serialized-actor invariant (same reasoning as the supervisor's name check).
        self.ensure_page_exists(&edge.page_a).await?;
        self.ensure_page_exists(&edge.page_b).await?;

        // A session origin must reference a real session: a stale id is a clean 404,
        // not the FK-violation 500 it would otherwise become at write time (symmetric
        // with the page pre-check above). The resolved ordinal also dates a supersede
        // against the row it replaces (below).
        let origin_ordinal = match &msg.origin {
            Origin::Prior => None,
            Origin::Session(s) => Some(
                self.session_ordinal(s)
                    .await?
                    .ok_or_else(|| CreateRelationshipError::SessionNotFound(s.clone()))?,
            ),
        };

        // A plain create is one `Create` write. A supersede also ends the named live
        // row in the same batch, create-first: a new fact that already exists live
        // trips the partial unique index on the insert and rolls the whole batch back,
        // so the old is never ended without its replacement (and the GM can fall back
        // to a plain End).
        let writes = match msg.supersedes {
            None => vec![RelationshipWrite::Create(edge.into_new(
                msg.visibility,
                msg.origin,
                msg.ending,
            ))],
            Some(old_id) => {
                let existing = self
                    .store
                    .get(&old_id)
                    .ok_or(CreateRelationshipError::SupersedesNotFound)?;
                if existing.invalidation.is_some() {
                    return Err(CreateRelationshipError::SupersedesNotLive);
                }
                // `edge` and `existing` are both canonical (page_a < page_b), so a
                // direct endpoint comparison decides "same fact".
                if existing.page_a != edge.page_a || existing.page_b != edge.page_b {
                    return Err(CreateRelationshipError::SupersedesDifferentPair);
                }
                // Capture the old row's origin before any further reader borrow of self.
                let old_origin = existing.origin.clone();

                // The new fact's origin session is also when the old ends, so a
                // supersede cannot originate before the campaign.
                let as_of = match &msg.origin {
                    Origin::Session(s) => s.clone(),
                    Origin::Prior => {
                        return Err(CreateRelationshipError::PriorOriginCannotSupersede);
                    }
                };

                // That origin session is when the old fact ends, so it cannot precede
                // the old fact's own origin (a fact cannot end before it began).
                if let Origin::Session(old_origin_s) = &old_origin {
                    let old_ordinal =
                        self.session_ordinal(old_origin_s).await?.ok_or_else(|| {
                            CreateRelationshipError::Db(sea_orm::DbErr::Custom(
                                "superseded row's origin session missing ordinal (FK invariant broken)"
                                    .into(),
                            ))
                        })?;
                    let new_ordinal = origin_ordinal
                        .expect("a supersede origin is a session (Prior rejected above)");
                    if new_ordinal < old_ordinal {
                        return Err(CreateRelationshipError::EndBeforeOrigin);
                    }
                }

                // The replacement births live (`ending` = None); the old row carries the
                // superseded invalidation.
                let new = edge.into_new(msg.visibility, msg.origin, None);
                vec![
                    RelationshipWrite::Create(new),
                    RelationshipWrite::Invalidate {
                        rel_id: old_id,
                        reason: InvalidationReason::Superseded,
                        by: Some(as_of),
                    },
                ]
            }
        };

        let outcomes = match self.db_writer.ask(ApplyRelationshipWrites { writes }).await {
            Ok(o) => o,
            Err(SendError::HandlerError(e)) => return Err(create_err_from_write(e)),
            Err(e) => {
                tracing::error!(error = %e, "db writer unavailable creating relationship");
                return Err(CreateRelationshipError::ActorUnavailable);
            }
        };

        // The `Create` is always the first write, so the first outcome is the new row;
        // reflect any remaining outcome (the invalidated old row, on supersede).
        let mut outcomes = outcomes.into_iter();
        let model = match outcomes.next() {
            Some(RelationshipWriteOutcome::Upserted(model)) => model,
            _ => {
                return Err(CreateRelationshipError::Db(sea_orm::DbErr::Custom(
                    "create batch returned no committed row".into(),
                )));
            }
        };
        let rel = relationship_from_model(model);
        self.store.upsert(rel.clone());
        self.reflect(outcomes.collect());
        Ok(self.view_for(&rel, &subject).await?)
    }
}

/// Apply an in-place op to an existing relationship. The widget refetches on success
/// (v1 live-update model), so the reply is just success/typed-failure.
#[derive(Debug, Clone)]
pub struct ApplyOp {
    pub rel_id: RelationshipId,
    pub op: RelationshipOp,
}

/// The in-place ops on an existing relationship. `End` carries the `SessionId` it
/// ended at; `SetVisibility` the new visibility; `Retcon`/`Delete` carry nothing.
/// Supersede is *not* here - it mints a new row, so it rides
/// [`CreateRelationship::supersedes`].
#[derive(Debug, Clone)]
pub enum RelationshipOp {
    End { as_of: SessionId },
    Retcon,
    Delete,
    SetVisibility { visibility: Visibility },
}

#[derive(Debug, thiserror::Error)]
pub enum ApplyOpError {
    #[error("relationship not found")]
    NotFound,
    #[error("a live relationship with this predicate pair already exists")]
    DuplicateLiveFact,
    #[error("relationship is already invalidated")]
    AlreadyInvalidated,
    #[error("referenced session not found: {0}")]
    SessionNotFound(SessionId),
    #[error("a relationship cannot end before it began")]
    EndBeforeOrigin,
    #[error("relationship write actor unavailable")]
    ActorUnavailable,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

impl Message<ApplyOp> for RelationshipGraph {
    type Reply = Result<(), ApplyOpError>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0, rel_id = %msg.rel_id.0),
    )]
    async fn handle(&mut self, msg: ApplyOp, _ctx: &mut Context<Self, Self::Reply>) -> Self::Reply {
        // Each GM op decomposes into an ordered list of single-statement writes the
        // writer runs in one transaction; the actor then reflects the committed
        // outcome(s) into the graph. Supersede is the only multi-write op.
        let writes: Vec<RelationshipWrite> = match msg.op {
            RelationshipOp::End { as_of } => {
                // The as-of session must exist (a stale id is a clean 404), and the end
                // cannot precede the fact's origin (a fact cannot end before it began).
                let as_of_ordinal = self
                    .session_ordinal(&as_of)
                    .await?
                    .ok_or_else(|| ApplyOpError::SessionNotFound(as_of.clone()))?;
                let origin = self
                    .store
                    .get(&msg.rel_id)
                    .ok_or(ApplyOpError::NotFound)?
                    .origin
                    .clone();
                if let Origin::Session(origin_s) = &origin {
                    let origin_ordinal =
                        self.session_ordinal(origin_s).await?.ok_or_else(|| {
                            ApplyOpError::Db(sea_orm::DbErr::Custom(
                                "origin session missing ordinal (FK invariant broken)".into(),
                            ))
                        })?;
                    if as_of_ordinal < origin_ordinal {
                        return Err(ApplyOpError::EndBeforeOrigin);
                    }
                }
                vec![RelationshipWrite::Invalidate {
                    rel_id: msg.rel_id,
                    reason: InvalidationReason::Superseded,
                    by: Some(as_of),
                }]
            }
            RelationshipOp::Retcon => vec![RelationshipWrite::Invalidate {
                rel_id: msg.rel_id,
                reason: InvalidationReason::Retconned,
                by: None,
            }],
            RelationshipOp::SetVisibility { visibility } => {
                vec![RelationshipWrite::SetVisibility {
                    rel_id: msg.rel_id,
                    visibility,
                }]
            }
            RelationshipOp::Delete => vec![RelationshipWrite::Delete { rel_id: msg.rel_id }],
        };

        let outcomes = match self.db_writer.ask(ApplyRelationshipWrites { writes }).await {
            Ok(o) => o,
            Err(SendError::HandlerError(e)) => return Err(apply_err_from_write(e)),
            Err(e) => {
                tracing::error!(error = %e, "db writer unavailable applying relationship writes");
                return Err(ApplyOpError::ActorUnavailable);
            }
        };
        self.reflect(outcomes);
        Ok(())
    }
}

/// The campaign's predicate vocabulary, harvested from the in-memory edges. Powers
/// the create modal's predicate typeahead + reverse autofill.
#[derive(Debug, Clone, Copy)]
pub struct KnownPredicatePairs;

impl Message<KnownPredicatePairs> for RelationshipGraph {
    type Reply = Vec<PredicatePair>;

    #[tracing::instrument(
        skip_all,
        fields(campaign_id = %self.campaign_id.0),
    )]
    async fn handle(
        &mut self,
        _: KnownPredicatePairs,
        _ctx: &mut Context<Self, Self::Reply>,
    ) -> Self::Reply {
        known_predicate_pairs(self.store.all_edges())
    }
}

fn create_err_from_write(e: RelationshipWriteError) -> CreateRelationshipError {
    match e {
        RelationshipWriteError::DuplicateLiveFact => CreateRelationshipError::DuplicateLiveFact,
        // Create never targets an existing row; a NotFound / AlreadyInvalidated here is
        // a logic error (the supersede path pre-checks `SupersedesNotLive`).
        RelationshipWriteError::NotFound => CreateRelationshipError::Db(sea_orm::DbErr::Custom(
            "unexpected NotFound creating relationship".into(),
        )),
        RelationshipWriteError::AlreadyInvalidated => CreateRelationshipError::Db(
            sea_orm::DbErr::Custom("unexpected AlreadyInvalidated creating relationship".into()),
        ),
        RelationshipWriteError::Db(e) => CreateRelationshipError::Db(e),
    }
}

fn apply_err_from_write(e: RelationshipWriteError) -> ApplyOpError {
    match e {
        RelationshipWriteError::DuplicateLiveFact => ApplyOpError::DuplicateLiveFact,
        RelationshipWriteError::NotFound => ApplyOpError::NotFound,
        RelationshipWriteError::AlreadyInvalidated => ApplyOpError::AlreadyInvalidated,
        RelationshipWriteError::Db(e) => ApplyOpError::Db(e),
    }
}
