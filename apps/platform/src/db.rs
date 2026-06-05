use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};

/// Connect to the platform SQLite DB, encoding our default connection
/// behaviour (foreign keys on) in one place.
pub async fn connect(database_url: &str) -> Result<DatabaseConnection, DbErr> {
    let mut opts = ConnectOptions::new(database_url);
    opts.map_sqlx_sqlite_opts(|o| o.foreign_keys(true));
    Database::connect(opts).await
}
