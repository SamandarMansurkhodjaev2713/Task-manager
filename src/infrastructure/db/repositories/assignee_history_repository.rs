use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::application::ports::repositories::{AssigneeHistoryEntry, AssigneeHistoryRepository};
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::AssigneeHistoryRow;

use super::common::database_error;

#[derive(Clone)]
pub struct SqliteAssigneeHistoryRepository {
    pool: SqlitePool,
}

impl SqliteAssigneeHistoryRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AssigneeHistoryRepository for SqliteAssigneeHistoryRepository {
    /// Upsert the `(creator_user_id, employee_id)` pair.  If the row exists,
    /// increments `use_count` and refreshes `last_used_at`.  The SQL is an
    /// efficient single-statement upsert using the unique index.
    async fn record_assignment(
        &self,
        creator_user_id: i64,
        employee_id: i64,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO assignee_history
                 (creator_user_id, employee_id, last_used_at, use_count)
             VALUES (?, ?, ?, 1)
             ON CONFLICT(creator_user_id, employee_id) DO UPDATE SET
                 use_count    = use_count + 1,
                 last_used_at = excluded.last_used_at",
        )
        .bind(creator_user_id)
        .bind(employee_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn top_for_creator(
        &self,
        creator_user_id: i64,
        limit: u32,
    ) -> AppResult<Vec<AssigneeHistoryEntry>> {
        let rows = sqlx::query_as::<_, AssigneeHistoryRow>(
            "SELECT id, creator_user_id, employee_id, last_used_at, use_count
             FROM assignee_history
             WHERE creator_user_id = ?
             ORDER BY use_count DESC, last_used_at DESC
             LIMIT ?",
        )
        .bind(creator_user_id)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(rows
            .into_iter()
            .map(|row| AssigneeHistoryEntry {
                employee_id: row.employee_id,
                use_count: row.use_count as u32,
                last_used_at: row.last_used_at,
            })
            .collect())
    }
}
