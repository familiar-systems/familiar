//! Schema consistency: two checks that keep the sea-orm entities and the
//! migrations honest. `cargo test` runs both; nothing to regenerate by hand.
//!
//! `entities_match_schema` compares the migrated DB against an entity-derived one
//! (the runtime-safety guarantee). `migrated_schema_matches_golden` snapshots the
//! realized DDL (`sqlite_master`) against the committed `tests/schema.sql`,
//! catching what the entity comparison structurally can't: CHECK constraints,
//! partial and non-unique indexes, defaults. The golden is *derived, not
//! authored* - recomputed from the same `Migrator` production runs and asserted
//! equal on every run, so it can't silently drift from the migrations; re-bless an
//! intentional change with `mise run bless-schema` and review the `schema.sql` diff.
//!
//! Against a freshly-migrated in-memory DB and an entity-derived in-memory DB,
//! `entities_match_schema` asserts: the set of migrated user tables (minus the
//! vec0 `block_embeddings` table and `sqlite_`/`seaql_` internals) equals the set
//! of entity `table_name()`s; and per table, the entity's columns (by name,
//! SQLite type affinity so `bigint`/`integer` don't false-positive, and
//! nullability), foreign keys, and unique constraints (incl. PRIMARY KEY) match
//! the migrated table, and `Entity::find().all()` executes against the live
//! schema. That is the entity<->migration runtime-safety guarantee.
//!
//! The *entity* comparison can't express CHECK constraints, partial/composite or
//! non-unique indexes, column defaults, or the vec0 table shape - sea-orm
//! entities have no syntax for them. Those are caught instead by the golden
//! snapshot, and where their *behavior* is load-bearing, by behavioral tests too
//! (the `campaign_metadata` `id = 1` CHECK in that migration's own test module;
//! the `relationships` partial unique "live fact" index in
//! `tests/relationships_test.rs`).

use std::collections::{BTreeMap, BTreeSet};

use familiar_systems_campaign::db;
use familiar_systems_campaign::entities::{
    blocks, campaign_metadata, pages, relationships, sessions, toc_entries,
};
use familiar_systems_campaign::migrations::Migrator;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, Schema, Statement,
};
use sea_orm_migration::MigratorTrait;

async fn setup_via_migrations() -> DatabaseConnection {
    db::register_sqlite_vec();
    let db = db::connect("sqlite::memory:").await.expect("connect");
    Migrator::up(&db, None).await.expect("migrate");
    db
}

/// SQLite type affinity, per the five documented rules. The only normalization we
/// own: it folds `bigint`/`integer` (same affinity, different spelling) together so
/// they never false-positive, while a real `int`<->`text` change still trips.
fn affinity(declared_type: &str) -> &'static str {
    let t = declared_type.to_uppercase();
    if t.contains("INT") {
        "INTEGER"
    } else if t.contains("CHAR") || t.contains("CLOB") || t.contains("TEXT") {
        "TEXT"
    } else if t.contains("BLOB") || t.is_empty() {
        "BLOB"
    } else if t.contains("REAL") || t.contains("FLOA") || t.contains("DOUB") {
        "REAL"
    } else {
        "NUMERIC"
    }
}

#[derive(Debug, PartialEq, Eq)]
struct ColAttr {
    affinity: &'static str,
    notnull: bool,
}

/// Column name -> `(affinity, notnull)` for `table`, read from the live DB.
async fn col_attrs(db: &DatabaseConnection, table: &str) -> BTreeMap<String, ColAttr> {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!("SELECT name, type, \"notnull\" FROM pragma_table_info('{table}')"),
        ))
        .await
        .expect("pragma_table_info");

    rows.into_iter()
        .map(|r| {
            let name: String = r.try_get("", "name").unwrap();
            let ty: String = r.try_get("", "type").unwrap();
            let notnull = r.try_get::<i32>("", "notnull").unwrap() != 0;
            (
                name,
                ColAttr {
                    affinity: affinity(&ty),
                    notnull,
                },
            )
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct FkAttrs {
    from: String,
    to_table: String,
    to: String,
    on_delete: String,
    on_update: String,
}

async fn foreign_keys(db: &DatabaseConnection, table: &str) -> Vec<FkAttrs> {
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

/// The set of UNIQUE column-groups for `table`. A `#[sea_orm(unique)]` column, a
/// migration `.unique_key()`, and PRIMARY KEY all surface here as unique indexes.
/// Non-unique (performance) indexes are excluded, since entities don't declare them.
/// PARTIAL unique indexes (`... WHERE <predicate>`) are excluded too: a sea-orm
/// entity can't express a partial or composite unique index, so comparing them
/// would always false-positive. Like CHECK constraints, they're migration-owned
/// and behaviorally tested (see `relationships`' live-fact index, exercised in
/// `tests/relationships_test.rs`). Index names differ between the two DBs, so we
/// key on the ordered column list.
async fn unique_indexes(db: &DatabaseConnection, table: &str) -> BTreeSet<Vec<String>> {
    let index_rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            format!(
                "SELECT name FROM pragma_index_list('{table}') \
                 WHERE \"unique\" = 1 AND \"partial\" = 0"
            ),
        ))
        .await
        .expect("pragma_index_list");

    let mut groups = BTreeSet::new();
    for row in index_rows {
        let name: String = row.try_get("", "name").unwrap();
        let col_rows = db
            .query_all(Statement::from_string(
                DatabaseBackend::Sqlite,
                format!("SELECT name FROM pragma_index_info('{name}') ORDER BY seqno"),
            ))
            .await
            .expect("pragma_index_info");
        let cols: Vec<String> = col_rows
            .into_iter()
            .map(|r| r.try_get("", "name").unwrap())
            .collect();
        groups.insert(cols);
    }
    groups
}

async fn user_tables(db: &DatabaseConnection) -> BTreeSet<String> {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT LIKE 'sqlite_%' \
             AND name NOT LIKE 'seaql_%' \
             AND name NOT LIKE 'block_embeddings%'"
                .to_owned(),
        ))
        .await
        .expect("sqlite_master");

    rows.into_iter()
        .map(|r| r.try_get::<String>("", "name").unwrap())
        .collect()
}

/// Materialize `entity`'s table in the entity-derived DB, then assert its columns
/// (name + affinity + nullability), foreign keys, and unique constraints match the
/// migrated table, and that a SELECT of every entity column executes against the
/// migrated schema. Returns the table name so the caller can assert full table-set
/// coverage.
///
/// `create_table_from_entity` tolerates FK targets that don't exist yet, so entities
/// can be applied in any order.
async fn check_entity<E: EntityTrait>(
    migrated: &DatabaseConnection,
    entity_db: &DatabaseConnection,
    schema: &Schema,
    backend: DatabaseBackend,
    entity: E,
) -> String {
    let table = entity.table_name().to_owned();

    let stmt = schema.create_table_from_entity(entity);
    entity_db
        .execute(backend.build(&stmt))
        .await
        .expect("create table from entity");

    assert_eq!(
        col_attrs(migrated, &table).await,
        col_attrs(entity_db, &table).await,
        "column drift (name, type affinity, or nullability) in `{table}` between entity and migration"
    );
    assert_eq!(
        foreign_keys(migrated, &table).await,
        foreign_keys(entity_db, &table).await,
        "foreign-key drift in `{table}` between entity and migration"
    );
    assert_eq!(
        unique_indexes(migrated, &table).await,
        unique_indexes(entity_db, &table).await,
        "unique-constraint drift in `{table}` between entity and migration"
    );
    E::find().all(migrated).await.unwrap_or_else(|e| {
        panic!("`{table}`: Entity::find().all() failed against the migrated schema: {e}")
    });

    table
}

#[tokio::test]
async fn entities_match_schema() {
    let migrated = setup_via_migrations().await;
    let entity_db = db::connect("sqlite::memory:").await.expect("connect");
    let schema = Schema::new(entity_db.get_database_backend());
    let backend = entity_db.get_database_backend();

    let covered: BTreeSet<String> = [
        check_entity(&migrated, &entity_db, &schema, backend, pages::Entity).await,
        check_entity(&migrated, &entity_db, &schema, backend, blocks::Entity).await,
        check_entity(
            &migrated,
            &entity_db,
            &schema,
            backend,
            campaign_metadata::Entity,
        )
        .await,
        check_entity(&migrated, &entity_db, &schema, backend, toc_entries::Entity).await,
        check_entity(&migrated, &entity_db, &schema, backend, sessions::Entity).await,
        check_entity(
            &migrated,
            &entity_db,
            &schema,
            backend,
            relationships::Entity,
        )
        .await,
    ]
    .into_iter()
    .collect();

    assert_eq!(
        user_tables(&migrated).await,
        covered,
        "table-set drift: every migrated user table (except the vec0 `block_embeddings`) \
         must have an entity, and vice versa"
    );
}

/// The realized schema as `sqlite_master` stores it after a full migration: every
/// `CREATE TABLE`/`CREATE INDEX` verbatim, so CHECK constraints, partial and
/// non-unique indexes, and defaults are all captured (no pragma exposes those).
/// `sql IS NOT NULL` drops the implicit `sqlite_autoindex_*` rows; `block_embeddings%`
/// is the vec0 virtual table + its shadow tables, whose DDL tracks the extension,
/// not us; `seaql_%` is sea-orm's migration bookkeeping.
async fn dump_schema(db: &DatabaseConnection) -> String {
    let rows = db
        .query_all(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT sql FROM sqlite_master \
             WHERE sql IS NOT NULL \
             AND name NOT LIKE 'sqlite_%' \
             AND name NOT LIKE 'seaql_%' \
             AND name NOT LIKE 'block_embeddings%' \
             ORDER BY type, name"
                .to_owned(),
        ))
        .await
        .expect("sqlite_master");
    let mut sql = rows
        .into_iter()
        .map(|r| r.try_get::<String>("", "sql").unwrap())
        .collect::<Vec<_>>()
        .join(";\n\n");
    sql.push_str(";\n");
    sql
}

/// Golden snapshot of the realized schema. Derived, not authored: recomputed from
/// the same `Migrator` production runs and asserted equal to `tests/schema.sql`
/// every run, so it can't silently drift from the migrations. This is where CHECK
/// constraints and partial indexes (invisible to `entities_match_schema`) get
/// caught. Re-bless an intentional change with `mise run bless-schema`.
#[tokio::test]
async fn migrated_schema_matches_golden() {
    let db = setup_via_migrations().await;
    let actual = dump_schema(&db).await;
    let golden = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/schema.sql");

    if std::env::var_os("UPDATE_SCHEMA_GOLDEN").is_some() {
        std::fs::write(golden, &actual).expect("write schema golden");
        eprintln!("blessed schema golden: {golden}");
        return;
    }

    let expected = std::fs::read_to_string(golden).unwrap_or_else(|_| {
        panic!(
            "missing {golden}; create it with `mise run bless-schema` \
             (or UPDATE_SCHEMA_GOLDEN=1 cargo test -p familiar-systems-campaign \
             --test schema migrated_schema_matches_golden)"
        )
    });
    assert_eq!(
        actual, expected,
        "schema drift: the migrated schema no longer matches tests/schema.sql. \
         If this change is intentional, re-bless with `mise run bless-schema` and \
         review the diff."
    );
}
