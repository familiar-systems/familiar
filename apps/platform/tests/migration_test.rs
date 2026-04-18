use familiar_systems_platform::migrations::Migrator;
use sea_orm::{ConnectionTrait, Database, Statement};
use sea_orm_migration::MigratorTrait;

#[tokio::test]
async fn migrator_creates_users_table_with_hanko_sub_unique() {
    let db = Database::connect("sqlite::memory:").await.unwrap();
    Migrator::up(&db, None).await.unwrap();
    let result = db
        .query_one(Statement::from_string(
            db.get_database_backend(),
            "select sql from sqlite_master where type='table' and name='users'".to_string(),
        ))
        .await
        .unwrap()
        .expect("users table not found");
    let sql: String = result.try_get("", "sql").unwrap();
    assert!(
        sql.contains("hanko_sub"),
        "expected hanko_sub column in DDL, got: {sql}"
    );
    assert!(
        sql.to_lowercase().contains("unique"),
        "expected UNIQUE constraint in DDL, got: {sql}"
    );
}
