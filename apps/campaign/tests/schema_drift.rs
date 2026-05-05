//! Integration test: the DB schema after applying all migrations must match
//! the schema each entity declares (via `Schema::create_table_from_entity`
//! and `Schema::create_index_from_entity`).
//!
//! Two DBs side-by-side, three pragma comparisons per table.
//!
//! Coverage:
//! - **Table-set**: every entity has a migrated table; every migrated table
//!   has an entity. Virtual `block_embeddings` is exempted (vec0 has no
//!   entity by design).
//! - **Columns**: name, declared type, nullability, primary-key flag.
//! - **Foreign keys**: from/to columns, target table, ON DELETE / ON UPDATE.
//! - **Indexes**: explicit indexes only (filtered to `origin = 'c'`); PK
//!   auto-indexes are excluded since both sides get them automatically.
//!
//! Not covered:
//! - **CHECK constraints**: sea-orm 1.1's entity `ColumnDef` has no CHECK
//!   field and the macro recognizes no CHECK-related attribute, so the
//!   entity literally cannot represent them. The CHECK we have
//!   (`campaign_metadata.id = 1`) is verified behaviorally by
//!   `id_other_than_one_violates_check_constraint` in the migration's own
//!   tests module.

use std::collections::BTreeSet;

use familiar_systems_campaign::db;
use familiar_systems_campaign::entities::{blocks, campaign_metadata, things};
use familiar_systems_campaign::migrations::Migrator;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbBackend, EntityName, EntityTrait,
    Schema, Statement,
};
use sea_orm_migration::MigratorTrait;

async fn setup_via_migrations() -> DatabaseConnection {
    db::register_sqlite_vec();
    let db = db::connect("sqlite::memory:").await.expect("connect");
    Migrator::up(&db, None).await.expect("migrate");
    db
}

async fn setup_via_entities() -> DatabaseConnection {
    let db = db::connect("sqlite::memory:").await.expect("connect");
    let schema = Schema::new(DbBackend::Sqlite);
    let backend = db.get_database_backend();

    // Apply in FK dependency order: parents before children.
    apply_entity_schema(&db, &schema, backend, things::Entity).await;
    apply_entity_schema(&db, &schema, backend, blocks::Entity).await;
    apply_entity_schema(&db, &schema, backend, campaign_metadata::Entity).await;

    db
}

async fn apply_entity_schema<E>(
    db: &DatabaseConnection,
    schema: &Schema,
    backend: DatabaseBackend,
    entity: E,
) where
    E: EntityTrait,
{
    let table_stmt = schema.create_table_from_entity(entity);
    db.execute(backend.build(&table_stmt))
        .await
        .expect("create table from entity");
    for index_stmt in schema.create_index_from_entity(entity) {
        db.execute(backend.build(&index_stmt))
            .await
            .expect("create index from entity");
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ColumnAttrs {
    name: String,
    declared_type: String,
    notnull: bool,
    pk: i32,
}

async fn pragma_columns(db: &DatabaseConnection, table: &str) -> Vec<ColumnAttrs> {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("SELECT name, type, \"notnull\", pk FROM pragma_table_info('{table}')"),
        ))
        .await
        .expect("pragma_table_info");

    let mut cols: Vec<ColumnAttrs> = rows
        .into_iter()
        .map(|r| ColumnAttrs {
            name: r.try_get("", "name").unwrap(),
            declared_type: r.try_get::<String>("", "type").unwrap().to_lowercase(),
            notnull: r.try_get::<i32>("", "notnull").unwrap() != 0,
            pk: r.try_get("", "pk").unwrap(),
        })
        .collect();
    cols.sort();
    cols
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FkAttrs {
    from: String,
    to_table: String,
    to: String,
    on_delete: String,
    on_update: String,
}

async fn pragma_foreign_keys(db: &DatabaseConnection, table: &str) -> Vec<FkAttrs> {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT \"table\", \"from\", \"to\", on_delete, on_update \
                 FROM pragma_foreign_key_list('{table}')"
            ),
        ))
        .await
        .expect("pragma_foreign_key_list");

    let mut fks: Vec<FkAttrs> = rows
        .into_iter()
        .map(|r| FkAttrs {
            from: r.try_get("", "from").unwrap(),
            to_table: r.try_get("", "table").unwrap(),
            to: r.try_get("", "to").unwrap(),
            on_delete: r.try_get::<String>("", "on_delete").unwrap().to_uppercase(),
            on_update: r.try_get::<String>("", "on_update").unwrap().to_uppercase(),
        })
        .collect();
    fks.sort();
    fks
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct IndexAttrs {
    columns: Vec<String>,
    unique: bool,
    partial: bool,
}

async fn pragma_indexes(db: &DatabaseConnection, table: &str) -> Vec<IndexAttrs> {
    // origin='c' means an explicit CREATE INDEX (vs 'pk' for primary-key
    // auto-indexes and 'u' for UNIQUE-constraint auto-indexes). We compare
    // only explicit indexes; the auto-indexes are deterministic from PK/UNIQUE
    // declarations already covered by pragma_columns.
    let index_rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT name, \"unique\", partial FROM pragma_index_list('{table}') \
                 WHERE origin = 'c'"
            ),
        ))
        .await
        .expect("pragma_index_list");

    let mut indexes = Vec::new();
    for row in index_rows {
        let name: String = row.try_get("", "name").unwrap();
        let col_rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("SELECT name FROM pragma_index_info('{name}') ORDER BY seqno"),
            ))
            .await
            .expect("pragma_index_info");
        let columns: Vec<String> = col_rows
            .into_iter()
            .map(|r| r.try_get("", "name").unwrap())
            .collect();

        indexes.push(IndexAttrs {
            columns,
            unique: row.try_get::<i32>("", "unique").unwrap() != 0,
            partial: row.try_get::<i32>("", "partial").unwrap() != 0,
        });
    }
    indexes.sort();
    indexes
}

async fn user_tables(db: &DatabaseConnection) -> BTreeSet<String> {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' \
             AND name NOT LIKE 'seaql_%' \
             AND name NOT LIKE 'block_embeddings%'"
                .to_string(),
        ))
        .await
        .expect("sqlite_master");

    rows.into_iter()
        .map(|r| r.try_get::<String>("", "name").unwrap())
        .collect()
}

#[tokio::test]
async fn schema_matches_entities() {
    let migrated = setup_via_migrations().await;
    let derived = setup_via_entities().await;

    assert_eq!(
        user_tables(&migrated).await,
        user_tables(&derived).await,
        "table-set drift between migrations and entities"
    );

    for table in [
        things::Entity.table_name(),
        blocks::Entity.table_name(),
        campaign_metadata::Entity.table_name(),
    ] {
        assert_eq!(
            pragma_columns(&migrated, table).await,
            pragma_columns(&derived, table).await,
            "column drift in `{table}`"
        );
        assert_eq!(
            pragma_foreign_keys(&migrated, table).await,
            pragma_foreign_keys(&derived, table).await,
            "FK drift in `{table}`"
        );
        assert_eq!(
            pragma_indexes(&migrated, table).await,
            pragma_indexes(&derived, table).await,
            "index drift in `{table}`"
        );
    }
}
