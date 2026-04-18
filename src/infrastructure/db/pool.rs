use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};
use sqlx::SqlitePool;

use crate::domain::errors::{AppError, AppResult};
use crate::shared::constants::timeouts::DATABASE_CONNECT_TIMEOUT_SECONDS;

pub async fn connect(database_url: &str) -> AppResult<SqlitePool> {
    // sqlx SQLite defaults to create_if_missing = false → код 14, если файла ещё нет.
    let url = database_url.trim().trim_end_matches('\r');
    let options = SqliteConnectOptions::from_str(url).map_err(|error| {
        AppError::internal(
            "DATABASE_URL_INVALID",
            "Invalid SQLite connection URL",
            serde_json::json!({ "error": error.to_string() }),
        )
    })?;
    // WAL mode: concurrent readers + writer without reader-writer contention.
    // synchronous=NORMAL is safe with WAL (no data loss on OS crash, only power failure).
    let options = options
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal);

    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .acquire_timeout(Duration::from_secs(DATABASE_CONNECT_TIMEOUT_SECONDS))
        .connect_with(options)
        .await
        .map_err(|error| {
            AppError::internal(
                "DATABASE_CONNECT_FAILED",
                "Failed to connect to SQLite",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .map_err(|error| {
            AppError::internal(
                "DATABASE_MIGRATION_FAILED",
                "Failed to apply database migrations",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;

    Ok(pool)
}
