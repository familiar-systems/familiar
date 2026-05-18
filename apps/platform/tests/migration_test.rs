use familiar_systems_platform::migrations::Migrator;
use sea_orm::{ConnectionTrait, Database, DatabaseConnection, Statement};
use sea_orm_migration::MigratorTrait;

async fn fresh_db() -> DatabaseConnection {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    db
}

async fn table_ddl(db: &DatabaseConnection, name: &str) -> String {
    let row = db
        .query_one(Statement::from_string(
            db.get_database_backend(),
            format!("select sql from sqlite_master where type='table' and name='{name}'"),
        ))
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{name} table not found"));
    row.try_get("", "sql").unwrap()
}

#[tokio::test]
async fn migrator_creates_users_table_with_expected_schema() {
    let db = fresh_db().await;
    let sql = table_ddl(&db, "users").await;

    assert!(
        sql.to_lowercase().contains("primary key"),
        "expected PRIMARY KEY in DDL, got: {sql}"
    );
    assert!(
        sql.contains("email"),
        "expected email column in DDL, got: {sql}"
    );
    assert!(
        sql.to_lowercase().contains("unique"),
        "expected UNIQUE constraint in DDL, got: {sql}"
    );
}

#[tokio::test]
async fn migrator_creates_campaigns_table_with_owner_fk_and_mirror_columns() {
    let db = fresh_db().await;
    let sql = table_ddl(&db, "campaigns").await;
    let lower = sql.to_lowercase();

    assert!(lower.contains("primary key"), "missing PK: {sql}");
    assert!(lower.contains("shard_url"), "missing shard_url: {sql}");
    assert!(
        lower.contains("owner_user_id"),
        "missing owner FK col: {sql}"
    );
    assert!(
        lower.contains("foreign key") && lower.contains("users"),
        "missing FK to users: {sql}"
    );
    for col in [
        "name",
        "tagline",
        "game_system",
        "content_locale",
        "last_init_error",
        "wizard_completed_at",
    ] {
        assert!(lower.contains(col), "missing {col}: {sql}");
    }
}

#[tokio::test]
async fn migrator_creates_create_attempts_table_with_pk_on_token_and_no_fk() {
    let db = fresh_db().await;
    let sql = table_ddl(&db, "create_attempts").await;
    let lower = sql.to_lowercase();

    assert!(
        lower.contains("idempotency_token") && lower.contains("primary key"),
        "expected idempotency_token PK: {sql}"
    );
    // Deliberately no FK to campaigns: the create flow writes the create_attempts
    // row before the campaigns row (ordering required for retry safety), so an
    // FK here would fail. See the migration's commentary.
    assert!(
        !lower.contains("foreign key"),
        "create_attempts must NOT have a FK constraint: {sql}"
    );
}
