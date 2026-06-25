//! Relationship write primitives: the per-statement SQL building blocks the
//! `ApplyRelationshipWrites` batch (in `database_writer_actor`) runs inside one
//! transaction. Dumb writes - invariant enforcement lives in the
//! `RelationshipGraph` actor; these reject only a duplicate live fact (the
//! partial unique index) or a missing row.

use chrono::{DateTime, Utc};

use familiar_systems_campaign_shared::id::{RelationshipId, SessionId};
use sea_orm::ActiveValue::Set;
use sea_orm::{ActiveModelTrait, DatabaseTransaction, EntityTrait, SqlErr};

use crate::domain::relationship::{Knowledge, NewRelationship};
use crate::entities::columns::{PageIdCol, RelationshipIdCol, SessionIdCol};
use crate::entities::relationships;

#[derive(Debug, thiserror::Error)]
pub enum RelationshipWriteError {
    #[error("a live relationship with this predicate pair already exists")]
    DuplicateLiveFact,
    #[error("relationship not found")]
    NotFound,
    #[error("database error: {0}")]
    Db(#[from] sea_orm::DbErr),
}

/// Narrow a write failure to `DuplicateLiveFact` when the partial unique index
/// (`idx_relationships_live_fact_unique`) trips. The live-fact index is the only
/// unique constraint a relationship write can violate (the PK is a freshly generated
/// ULID), so a unique violation here is unambiguously a duplicate live fact. Applied
/// to every write: a `Create` collides when it births a fact that already lives, and
/// a clearing `SetSuperseded { at: None }` / `SetRetcon { at: None }` collides when
/// it *re-adds* a row to the live set whose canonical fact is already live (un-ending
/// a row whose supersede minted a same-pair successor).
fn map_relationship_write_error(e: sea_orm::DbErr) -> RelationshipWriteError {
    match e.sql_err() {
        Some(SqlErr::UniqueConstraintViolation(_)) => RelationshipWriteError::DuplicateLiveFact,
        _ => RelationshipWriteError::Db(e),
    }
}

// The relationship primitives below operate on a `&DatabaseTransaction`, not
// `&self.conn`: they are the per-statement building blocks the `ApplyRelationshipWrites`
// batch interprets inside one transaction. The actor never calls them directly. They
// are *dumb* writes - all invariant enforcement (ordering, supersede pre-checks) lives
// in the actor; the only thing the writer rejects is a duplicate live fact (the index)
// and a missing row.

/// Insert a brand-new relationship row inside `txn`, stamping `created_at` from `now`
/// (the id is minted by the owning actor and arrives on `NewRelationship`). The two
/// axes come straight off `NewRelationship` (a born-finalized
/// retrofit can already carry `superseded`/`retcon`). A row that births already
/// superseded or retconned sits outside the live set, so it never collides; a live
/// birth that duplicates an existing live fact trips the index.
pub(super) async fn insert_relationship(
    txn: &DatabaseTransaction,
    new: NewRelationship,
    now: DateTime<Utc>,
) -> Result<relationships::Model, RelationshipWriteError> {
    relationships::ActiveModel {
        id: Set(RelationshipIdCol::from(new.id)),
        page_a: Set(PageIdCol::from(new.page_a)),
        page_b: Set(PageIdCol::from(new.page_b)),
        predicate_a_to_b: Set(new.predicate_a_to_b),
        predicate_b_to_a: Set(new.predicate_b_to_a),
        origin_session_id: Set(new.origin.session_id().map(SessionIdCol::from)),
        superseded_session_id: Set(new.superseded.map(SessionIdCol::from)),
        retcon_session_id: Set(new.retcon.map(SessionIdCol::from)),
        is_secret: Set(new.knowledge.is_secret()),
        reveal_session_id: Set(new.knowledge.reveal_session_id().map(SessionIdCol::from)),
        created_at: Set(now),
    }
    .insert(txn)
    .await
    .map_err(map_relationship_write_error)
}

/// Set (or clear) one nullable session-stamp column on a relationship row inside
/// `txn`: a blind UPDATE, no one-way-door guard (invalidation is reversible). A
/// clearing write (`value = None`) can re-add the row to the live set and so trip the
/// live-fact index, surfaced as `DuplicateLiveFact`. `column` selects which axis.
pub(super) async fn set_session_stamp(
    txn: &DatabaseTransaction,
    rel_id: RelationshipId,
    column: StampColumn,
    value: Option<SessionId>,
) -> Result<relationships::Model, RelationshipWriteError> {
    let existing = relationships::Entity::find_by_id(RelationshipIdCol::from(rel_id))
        .one(txn)
        .await?
        .ok_or(RelationshipWriteError::NotFound)?;
    let mut am: relationships::ActiveModel = existing.into();
    let stamp = value.map(SessionIdCol::from);
    match column {
        StampColumn::Superseded => am.superseded_session_id = Set(stamp),
        StampColumn::Retcon => am.retcon_session_id = Set(stamp),
    }
    am.update(txn).await.map_err(map_relationship_write_error)
}

/// Which nullable factuality session-stamp axis a [`RelationshipWrite::SetStamp`]
/// targets. (Knowledge is *not* a stamp: it sets two columns wholesale, via
/// `RelationshipWrite::SetKnowledge`.)
#[derive(Debug, Clone, Copy)]
pub enum StampColumn {
    Superseded,
    Retcon,
}

/// Set a relationship's knowledge wholesale inside `txn`: a blind UPDATE of both
/// `is_secret` and `reveal_session_id` from a [`Knowledge`] value. The pair is always
/// legal by construction (`Public` -> `(false, NULL)`, never `(false, Some)`), so this
/// can never trip the CHECK. Knowledge is not in the live-fact index, so it never trips
/// the uniqueness index either - the only failure is a missing row.
pub(super) async fn set_knowledge(
    txn: &DatabaseTransaction,
    rel_id: RelationshipId,
    knowledge: Knowledge,
) -> Result<relationships::Model, RelationshipWriteError> {
    let existing = relationships::Entity::find_by_id(RelationshipIdCol::from(rel_id))
        .one(txn)
        .await?
        .ok_or(RelationshipWriteError::NotFound)?;
    let mut am: relationships::ActiveModel = existing.into();
    am.is_secret = Set(knowledge.is_secret());
    am.reveal_session_id = Set(knowledge.reveal_session_id().map(SessionIdCol::from));
    am.update(txn).await.map_err(map_relationship_write_error)
}

/// Hard-delete a relationship row inside `txn`, no audit trail.
pub(super) async fn delete_relationship(
    txn: &DatabaseTransaction,
    rel_id: RelationshipId,
) -> Result<(), RelationshipWriteError> {
    let res = relationships::Entity::delete_by_id(RelationshipIdCol::from(rel_id))
        .exec(txn)
        .await?;
    if res.rows_affected == 0 {
        return Err(RelationshipWriteError::NotFound);
    }
    Ok(())
}

/// One relationship mutation: the building block the
/// [`RelationshipGraph`](crate::actors::relationship_graph::RelationshipGraph)
/// composes into an atomic [`ApplyRelationshipWrites`] batch. Each variant is one
/// SQL statement; the writer is a dumb interpreter that runs a list of them in a
/// single transaction. `SetStamp` covers the two reversible factuality axes
/// (superseded / retcon): `at = Some` stamps it, `None` clears it. `SetKnowledge` sets
/// the knowledge pair wholesale.
pub enum RelationshipWrite {
    Create(NewRelationship),
    SetStamp {
        rel_id: RelationshipId,
        column: StampColumn,
        at: Option<SessionId>,
    },
    SetKnowledge {
        rel_id: RelationshipId,
        knowledge: Knowledge,
    },
    Delete {
        rel_id: RelationshipId,
    },
}

/// What one [`RelationshipWrite`] committed, threaded back so the actor reflects it
/// into the in-memory graph. `Create`/`SetStamp` yield the committed row (the actor
/// upserts it by id); `Delete` yields the removed id. The row is boxed so the two
/// variants are similar-sized (a `Model` is ~240 bytes; an id is 16).
#[derive(Debug)]
pub enum RelationshipWriteOutcome {
    Upserted(Box<relationships::Model>),
    Removed(RelationshipId),
}
