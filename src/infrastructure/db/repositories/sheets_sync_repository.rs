use chrono::{DateTime, Duration as ChronoDuration, Utc};
use sqlx::SqlitePool;

use crate::application::ports::repositories::{SheetsSyncRepository, SheetsSyncRow};
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::PendingSheetWriteRow;

use super::common::{database_error, PENDING_SHEET_WRITE_COLUMNS};

/// Maximum per-row backoff (4 hours).  After `MAX_WRITE_BACK_ATTEMPTS` the
/// row is permanently skipped — this cap only affects how long we wait between
/// retries while the row still has attempts left.
const MAX_BACKOFF_MINUTES: i64 = 240;

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

    async fn list_pending(
        &self,
        max_attempts: u32,
        limit: i64,
        now: DateTime<Utc>,
    ) -> AppResult<Vec<SheetsSyncRow>> {
        // F-09: skip rows whose exponential back-off window has not expired.
        let query = format!(
            "SELECT {PENDING_SHEET_WRITE_COLUMNS}
             FROM pending_sheet_writes
             WHERE written_at IS NULL
               AND error_count < ?
               AND (next_attempt_at IS NULL OR next_attempt_at <= ?)
             ORDER BY created_at ASC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, PendingSheetWriteRow>(&query)
            .bind(max_attempts as i64)
            .bind(now)
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

    /// Increment `error_count`, store the error string, and compute
    /// `next_attempt_at` as an exponential back-off:
    ///
    /// ```text
    /// delay = min(2^error_count minutes, MAX_BACKOFF_MINUTES)
    /// next_attempt_at = now + delay
    /// ```
    ///
    /// `error_count` here is the **pre-increment** count, matching the number
    /// of retries already exhausted.  The first failure (error_count = 0)
    /// backs off 1 min; second → 2 min; third → 4 min; … cap → 4 h.
    async fn record_error(&self, id: i64, error: &str, now: DateTime<Utc>) -> AppResult<()> {
        // Read current error_count to compute backoff before updating.
        let current: i64 =
            sqlx::query_scalar("SELECT error_count FROM pending_sheet_writes WHERE id = ?")
                .bind(id)
                .fetch_one(&self.pool)
                .await
                .map_err(database_error)?;

        // delay = min(2^error_count, MAX_BACKOFF_MINUTES) minutes
        let backoff_minutes = std::cmp::min(
            1_i64.checked_shl(current as u32).unwrap_or(i64::MAX),
            MAX_BACKOFF_MINUTES,
        );
        let next_attempt_at = now + ChronoDuration::minutes(backoff_minutes);

        sqlx::query(
            "UPDATE pending_sheet_writes
             SET error_count     = error_count + 1,
                 last_error      = ?,
                 next_attempt_at = ?
             WHERE id = ?",
        )
        .bind(error)
        .bind(next_attempt_at)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }
}
