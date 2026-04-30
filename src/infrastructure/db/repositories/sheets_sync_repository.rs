use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::application::ports::repositories::{SheetsSyncRepository, SheetsSyncRow};
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::PendingSheetWriteRow;

use super::common::{database_error, PENDING_SHEET_WRITE_COLUMNS};

#[derive(Clone)]
pub struct SqliteSheetsSyncRepository {
    pool: SqlitePool,
}

impl SqliteSheetsSyncRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl SheetsSyncRepository for SqliteSheetsSyncRepository {
    async fn enqueue(
        &self,
        employee_id: i64,
        telegram_id: i64,
        full_name: &str,
        telegram_username: Option<&str>,
    ) -> AppResult<()> {
        // INSERT OR IGNORE: if a row already exists for this employee_id we
        // leave it untouched (the unique index on employee_id prevents dupes).
        sqlx::query(
            "INSERT OR IGNORE INTO pending_sheet_writes
                 (employee_id, telegram_id, full_name, telegram_username)
             VALUES (?, ?, ?, ?)",
        )
        .bind(employee_id)
        .bind(telegram_id)
        .bind(full_name)
        .bind(telegram_username)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn list_pending(&self, max_attempts: u32, limit: i64) -> AppResult<Vec<SheetsSyncRow>> {
        let query = format!(
            "SELECT {PENDING_SHEET_WRITE_COLUMNS}
             FROM pending_sheet_writes
             WHERE written_at IS NULL AND error_count < ?
             ORDER BY created_at ASC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, PendingSheetWriteRow>(&query)
            .bind(max_attempts as i64)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn mark_written(&self, id: i64, now: DateTime<Utc>) -> AppResult<()> {
        sqlx::query("UPDATE pending_sheet_writes SET written_at = ? WHERE id = ?")
            .bind(now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(())
    }

    async fn record_error(&self, id: i64, error: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE pending_sheet_writes
             SET error_count = error_count + 1,
                 last_error  = ?
             WHERE id = ?",
        )
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }
}
