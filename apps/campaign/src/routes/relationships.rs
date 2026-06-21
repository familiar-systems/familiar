//! Relationship REST surface: the GM curation endpoints the widget calls.
//!
//! Resource-oriented over `/campaign/{id}/relationships`: `POST` creates (an
//! optional `supersedes` pointer makes it an atomic replace), `PATCH` mutates the
//! visibility/invalidation a relationship is allowed to change, `DELETE` hard-deletes,
//! and `GET /pages/{pageId}/relationships` reads the per-page oriented view. The
//! predicate vocabulary, the session pickers, and the entity typeahead are the three
//! supporting reads. All GM-only; every handler resolves the supervisor via
//! `authorize_gm` and forwards to the `RelationshipGraph` (mutations + the page view +
//! predicates) or reads the table directly (sessions + entity search).

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use kameo::error::SendError;
use serde::Deserialize;

use familiar_systems_campaign_shared::id::{PageId, RelationshipId};
use familiar_systems_campaign_shared::relationship::{
    CreateRelationshipRequest, EntitySearchResult, InvalidationReason, OriginInput,
    PatchRelationshipRequest, PredicatePairView, RelationshipView, SessionRef, SessionsResponse,
};

use crate::actors::relationship_graph::{
    ApplyOp, ApplyOpError, CreateRelationship, CreateRelationshipError, KnownPredicatePairs,
    RelationshipOp, RelationshipsForPage,
};
use crate::actors::supervisor::{ListSessions, SearchEntities};
use crate::domain::relationship::Origin;
use crate::middleware::auth::{AuthenticatedUser, authorize_gm};
use crate::state::AppState;

/// How many entity matches the typeahead asks for. Small: it is a picker, not a list.
const ENTITY_SEARCH_LIMIT: u64 = 10;

// ---------------------------------------------------------------------------
// GET /campaign/{id}/pages/{pageId}/relationships
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/campaign/{id}/pages/{pageId}/relationships",
    tag = "relationships",
    params(
        ("id" = String, Path, description = "Campaign ID"),
        ("pageId" = String, Path, description = "Page whose relationships to read"),
    ),
    responses(
        (status = OK, description = "The page's relationships, oriented to it", body = Vec<RelationshipView>),
        (status = BAD_REQUEST, description = "Malformed page id"),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Read failed"),
    ),
)]
pub async fn get_relationships(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path((campaign_id, page_id)): Path<(String, String)>,
) -> Response {
    let page_id = match parse_page_id(&page_id) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Malformed page id.").into_response(),
    };
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };
    match supervisor.ask(RelationshipsForPage { page_id }).await {
        Ok(views) => (StatusCode::OK, Json(views)).into_response(),
        Err(e) => read_error(e, "get_relationships"),
    }
}

// ---------------------------------------------------------------------------
// POST /campaign/{id}/relationships
// ---------------------------------------------------------------------------

#[utoipa::path(
    post,
    path = "/campaign/{id}/relationships",
    tag = "relationships",
    params(("id" = String, Path, description = "Campaign ID")),
    request_body = CreateRelationshipRequest,
    responses(
        (status = CREATED, description = "Relationship created, oriented to the subject", body = RelationshipView),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard, an endpoint page is missing, a referenced session is missing, or the superseded relationship does not exist"),
        (status = CONFLICT, description = "A live relationship with this predicate pair already exists, or the superseded row is already invalidated"),
        (status = UNPROCESSABLE_ENTITY, description = "Self-edge, empty predicate, or an invalid supersede (different pair / prior origin / takes effect before the replaced fact began)"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Creation failed"),
    ),
)]
pub async fn create_relationship(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Json(req): Json<CreateRelationshipRequest>,
) -> Response {
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };
    let msg = CreateRelationship {
        subject: req.subject_page_id,
        other: req.other_page_id,
        predicate_forward: req.predicate_forward,
        predicate_reverse: req.predicate_reverse,
        visibility: req.visibility,
        origin: to_origin(req.origin),
        // The v1 create UI births live; a born-finalized retrofit is not exposed here.
        ending: None,
        supersedes: req.supersedes,
    };
    match supervisor.ask(msg).await {
        Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
        Err(e) => create_relationship_error(e),
    }
}

// ---------------------------------------------------------------------------
// PATCH /campaign/{id}/relationships/{relId}
// ---------------------------------------------------------------------------

#[utoipa::path(
    patch,
    path = "/campaign/{id}/relationships/{relId}",
    tag = "relationships",
    params(
        ("id" = String, Path, description = "Campaign ID"),
        ("relId" = String, Path, description = "Relationship ID"),
    ),
    request_body = PatchRelationshipRequest,
    responses(
        (status = NO_CONTENT, description = "Applied"),
        (status = BAD_REQUEST, description = "Malformed relationship id"),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard, relationship not found, or a referenced session is missing"),
        (status = CONFLICT, description = "The relationship is already invalidated"),
        (status = UNPROCESSABLE_ENTITY, description = "Empty patch, ending without an as-of session, or an end before the fact's origin"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Patch failed"),
    ),
)]
pub async fn patch_relationship(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path((campaign_id, rel_id)): Path<(String, String)>,
    Json(req): Json<PatchRelationshipRequest>,
) -> Response {
    let rel_id = match parse_rel_id(&rel_id) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Malformed relationship id.").into_response(),
    };
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };
    let ops = match patch_to_ops(rel_id, &req) {
        Ok(ops) => ops,
        Err(msg) => return (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response(),
    };
    // A single-field patch is one op. A both-fields patch (end/retcon *and* re-hide)
    // applies as two ops, which is not atomic; acceptable for single-GM v1 (the
    // combination is rare and recoverable on refetch).
    for op in ops {
        if let Err(e) = supervisor.ask(op).await {
            return apply_op_error(e);
        }
    }
    StatusCode::NO_CONTENT.into_response()
}

// ---------------------------------------------------------------------------
// DELETE /campaign/{id}/relationships/{relId}
// ---------------------------------------------------------------------------

#[utoipa::path(
    delete,
    path = "/campaign/{id}/relationships/{relId}",
    tag = "relationships",
    params(
        ("id" = String, Path, description = "Campaign ID"),
        ("relId" = String, Path, description = "Relationship ID"),
    ),
    responses(
        (status = NO_CONTENT, description = "Hard-deleted"),
        (status = BAD_REQUEST, description = "Malformed relationship id"),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard, or relationship not found"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Delete failed"),
    ),
)]
pub async fn delete_relationship(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path((campaign_id, rel_id)): Path<(String, String)>,
) -> Response {
    let rel_id = match parse_rel_id(&rel_id) {
        Some(id) => id,
        None => return (StatusCode::BAD_REQUEST, "Malformed relationship id.").into_response(),
    };
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };
    match supervisor
        .ask(ApplyOp {
            rel_id,
            op: RelationshipOp::Delete,
        })
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => apply_op_error(e),
    }
}

// ---------------------------------------------------------------------------
// GET /campaign/{id}/relationships/predicates
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/campaign/{id}/relationships/predicates",
    tag = "relationships",
    params(("id" = String, Path, description = "Campaign ID")),
    responses(
        (status = OK, description = "Known predicate pairs with usage counts", body = Vec<PredicatePairView>),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Read failed"),
    ),
)]
pub async fn known_predicates(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
) -> Response {
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };
    match supervisor.ask(KnownPredicatePairs).await {
        Ok(pairs) => {
            let views: Vec<PredicatePairView> = pairs
                .into_iter()
                .map(|p| PredicatePairView {
                    forward: p.forward,
                    reverse: p.reverse,
                    count: p.count as u32,
                })
                .collect();
            (StatusCode::OK, Json(views)).into_response()
        }
        Err(e) => read_error(e, "known_predicates"),
    }
}

// ---------------------------------------------------------------------------
// GET /campaign/{id}/sessions
// ---------------------------------------------------------------------------

#[utoipa::path(
    get,
    path = "/campaign/{id}/sessions",
    tag = "relationships",
    params(("id" = String, Path, description = "Campaign ID")),
    responses(
        (status = OK, description = "Sessions ascending by ordinal, plus the current one", body = SessionsResponse),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Read failed"),
    ),
)]
pub async fn list_sessions(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
) -> Response {
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };
    match supervisor.ask(ListSessions).await {
        Ok(rows) => {
            let sessions: Vec<SessionRef> = rows
                .into_iter()
                .map(|(id, ordinal)| SessionRef { id, ordinal })
                .collect();
            let current = sessions.iter().max_by_key(|s| s.ordinal).cloned();
            (StatusCode::OK, Json(SessionsResponse { sessions, current })).into_response()
        }
        Err(e) => read_error(e, "list_sessions"),
    }
}

// ---------------------------------------------------------------------------
// GET /campaign/{id}/entities?q=
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct EntitySearchParams {
    /// Substring to match against entity page titles.
    pub q: String,
}

#[utoipa::path(
    get,
    path = "/campaign/{id}/entities",
    tag = "relationships",
    params(
        ("id" = String, Path, description = "Campaign ID"),
        ("q" = String, Query, description = "Substring to match entity names"),
    ),
    responses(
        (status = OK, description = "Matching entity pages (excludes templates)", body = Vec<EntitySearchResult>),
        (status = UNAUTHORIZED, description = "Missing or invalid session"),
        (status = FORBIDDEN, description = "Caller is not a GM of this campaign"),
        (status = NOT_FOUND, description = "Campaign not on this shard"),
        (status = SERVICE_UNAVAILABLE, description = "Server restarting or platform unreachable"),
        (status = INTERNAL_SERVER_ERROR, description = "Read failed"),
    ),
)]
pub async fn search_entities(
    user: AuthenticatedUser,
    State(state): State<AppState>,
    Path(campaign_id): Path<String>,
    Query(params): Query<EntitySearchParams>,
) -> Response {
    let (_campaign_id, supervisor) = match authorize_gm(&state, campaign_id, &user).await {
        Ok(resolved) => resolved,
        Err(resp) => return resp,
    };
    match supervisor
        .ask(SearchEntities {
            query: params.q,
            limit: ENTITY_SEARCH_LIMIT,
        })
        .await
    {
        Ok(rows) => {
            let results: Vec<EntitySearchResult> = rows
                .into_iter()
                .map(|(id, name)| EntitySearchResult { id, name })
                .collect();
            (StatusCode::OK, Json(results)).into_response()
        }
        Err(e) => read_error(e, "search_entities"),
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn to_origin(o: OriginInput) -> Origin {
    match o {
        OriginInput::Prior => Origin::Prior,
        OriginInput::Session(s) => Origin::Session(s),
    }
}

/// Decompose a patch into the ops it implies, rejecting an empty patch and an
/// end-without-as-of. Predicates/origin are immutable, so only visibility and the
/// invalidation transition can appear.
/// On `Err`, the 422 body message (both failure modes are unprocessable). Returned
/// as a `&str` rather than a `Response` to keep the `Err` variant small (clippy's
/// `result_large_err`); the caller stamps the status.
fn patch_to_ops(
    rel_id: RelationshipId,
    req: &PatchRelationshipRequest,
) -> Result<Vec<ApplyOp>, &'static str> {
    let mut ops = Vec::new();
    if let Some(visibility) = req.visibility {
        ops.push(ApplyOp {
            rel_id: rel_id.clone(),
            op: RelationshipOp::SetVisibility { visibility },
        });
    }
    if let Some(inv) = &req.invalidation {
        let op = match inv.reason {
            InvalidationReason::Superseded => {
                let as_of = inv
                    .as_of
                    .clone()
                    .ok_or("Ending a relationship requires an as-of session.")?;
                RelationshipOp::End { as_of }
            }
            InvalidationReason::Retconned => RelationshipOp::Retcon,
        };
        ops.push(ApplyOp { rel_id, op });
    }
    if ops.is_empty() {
        return Err("Patch must change visibility or invalidation.");
    }
    Ok(ops)
}

fn parse_page_id(s: &str) -> Option<PageId> {
    ulid::Ulid::from_string(s).ok().map(PageId::from)
}

fn parse_rel_id(s: &str) -> Option<RelationshipId> {
    ulid::Ulid::from_string(s).ok().map(RelationshipId::from)
}

/// Map a create failure to a response. Each variant maps to a status by name; the
/// 500-collapsing variant is named (no `_` catch-all), so a new error is a compile
/// error rather than a silent 500. Mirrors `routes/pages.rs`.
fn create_relationship_error(
    e: SendError<CreateRelationship, CreateRelationshipError>,
) -> Response {
    rel_error_response(e, "create_relationship", |err| match err {
        CreateRelationshipError::SelfEdge | CreateRelationshipError::EmptyPredicate => {
            Some((StatusCode::UNPROCESSABLE_ENTITY, err.to_string()))
        }
        CreateRelationshipError::PageNotFound(_)
        | CreateRelationshipError::SupersedesNotFound
        | CreateRelationshipError::SessionNotFound(_) => {
            Some((StatusCode::NOT_FOUND, err.to_string()))
        }
        CreateRelationshipError::DuplicateLiveFact | CreateRelationshipError::SupersedesNotLive => {
            Some((StatusCode::CONFLICT, err.to_string()))
        }
        CreateRelationshipError::SupersedesDifferentPair
        | CreateRelationshipError::PriorOriginCannotSupersede
        | CreateRelationshipError::EndBeforeOrigin => {
            Some((StatusCode::UNPROCESSABLE_ENTITY, err.to_string()))
        }
        CreateRelationshipError::ActorUnavailable => {
            Some((StatusCode::SERVICE_UNAVAILABLE, err.to_string()))
        }
        CreateRelationshipError::Db(_) => None,
    })
}

fn apply_op_error(e: SendError<ApplyOp, ApplyOpError>) -> Response {
    rel_error_response(e, "apply_op", |err| match err {
        ApplyOpError::NotFound | ApplyOpError::SessionNotFound(_) => {
            Some((StatusCode::NOT_FOUND, err.to_string()))
        }
        ApplyOpError::DuplicateLiveFact | ApplyOpError::AlreadyInvalidated => {
            Some((StatusCode::CONFLICT, err.to_string()))
        }
        ApplyOpError::EndBeforeOrigin => Some((StatusCode::UNPROCESSABLE_ENTITY, err.to_string())),
        ApplyOpError::ActorUnavailable => Some((StatusCode::SERVICE_UNAVAILABLE, err.to_string())),
        ApplyOpError::Db(_) => None,
    })
}

/// Shared shape for the typed mutation errors: the classifier handles the arms a
/// path cares about, everything else (and a transport failure) is a logged 500.
fn rel_error_response<M, E: std::fmt::Display>(
    e: SendError<M, E>,
    context: &'static str,
    classify: impl Fn(&E) -> Option<(StatusCode, String)>,
) -> Response {
    match e {
        SendError::HandlerError(err) => match classify(&err) {
            Some((status, body)) => (status, body).into_response(),
            None => {
                tracing::error!(error = %err, context, "relationship mutation failed");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        },
        other => {
            tracing::error!(error = %other, context, "supervisor unreachable during relationship mutation");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// Reads surface only `sea_orm::DbErr` (or a transport failure) - both opaque to the
/// client, so both log and 500.
fn read_error<M>(e: SendError<M, sea_orm::DbErr>, context: &'static str) -> Response {
    tracing::error!(error = %e, context, "relationship read failed");
    StatusCode::INTERNAL_SERVER_ERROR.into_response()
}
