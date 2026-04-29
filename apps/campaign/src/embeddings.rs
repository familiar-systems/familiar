//! Vector search over `block_embeddings`. The `MATCH` operator and `k = ?`
//! predicate are vec0-specific so sea-orm's typed query builder can't model
//! them — this is the local escape hatch into hand-rolled SQL. Public surface
//! takes/returns branded `BlockId`s; the SQL is bounded to one method per
//! query shape.

use familiar_systems_campaign_shared::id::BlockId;
use sea_orm::{DatabaseConnection, FromQueryResult, Statement, Value};

#[derive(Debug, Clone, Copy)]
pub enum ViewerKind {
    Gm,
    Player,
}

pub struct EmbeddingsRepo<'a> {
    pub db: &'a DatabaseConnection,
}

#[derive(Debug, FromQueryResult)]
struct Row {
    block_id: String,
    distance: f32,
}

impl<'a> EmbeddingsRepo<'a> {
    pub async fn search(
        &self,
        query: &[f32],
        viewer: ViewerKind,
        k: u32,
    ) -> Result<Vec<(BlockId, f32)>, sea_orm::DbErr> {
        // Static fragments only — not user input. The visibility predicate
        // lives at the Thing level (per docs/plans/2026-02-22-ai-prd.md §2.2);
        // block-level redaction inside an otherwise-visible thing happens at
        // document materialization, not in SQL.
        let status_clause = match viewer {
            ViewerKind::Gm => "",
            ViewerKind::Player => "AND status NOT IN ('gm_only', 'retconned')",
        };
        let sql = format!(
            "SELECT block_id, distance FROM block_embeddings \
             WHERE embedding MATCH ?1 AND k = ?2 {status_clause} \
             ORDER BY distance"
        );
        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Sqlite,
            sql,
            [
                Value::Bytes(Some(Box::new(vec_to_bytes(query)))),
                Value::BigInt(Some(i64::from(k))),
            ],
        );
        let rows = Row::find_by_statement(stmt).all(self.db).await?;
        rows.into_iter()
            .map(|r| {
                let ulid = ulid::Ulid::from_string(&r.block_id).map_err(|e| {
                    sea_orm::DbErr::Custom(format!(
                        "block_embeddings.block_id is not a valid ULID: {e}"
                    ))
                })?;
                Ok((BlockId(ulid), r.distance))
            })
            .collect()
    }
}

/// Encode a `[f32]` as little-endian bytes for the vec0 `MATCH` parameter.
/// vec0 accepts JSON arrays too, but the binary form is the documented fast
/// path and avoids float-formatting round-trip surprises.
fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for f in v {
        out.extend_from_slice(&f.to_le_bytes());
    }
    out
}
