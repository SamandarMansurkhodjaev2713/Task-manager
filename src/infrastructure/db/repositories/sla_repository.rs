//! SQLite adapter for the SLA escalation worker.
//!
//! Writes are intentionally narrow — only the two SLA columns on `tasks`
//! (`sla_state`, `sla_last_level`) and the `sla_escalations` side-table are
//! ever touched here.  The full `Task` aggregate remains owned by
//! `SqliteTaskRepository`.

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::application::ports::repositories::{SlaRepository, SlaTaskRow};
use crate::domain::errors::AppResult;

use super::common::database_error;

#[derive(Clone)]
pub struct SqliteSlaRepository {
    pool: SqlitePool,
}

impl SqliteSlaRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl SlaRepository for SqliteSlaRepository {
    async fn list_active_with_deadline(&self, limit: i64) -> AppResult<Vec<SlaTaskRow>> {
        // Select only open tasks that have a concrete deadline.  Terminal
        // statuses (completed / cancelled) are excluded so the worker skips
        // rows that no longer need SLA tracking.
        let rows = sqlx::query(
            "SELECT id, task_uid, title, deadline, created_at,
                    assigned_to_user_id, sla_state, sla_last_level
             FROM tasks
             WHERE deadline IS NOT NULL
               AND status NOT IN ('completed', 'cancelled')
             ORDER BY deadline ASC
             LIMIT ?",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;

        let mut result = Vec::with_capacity(rows.len());
        for row in rows {
            let task_uid_str: String = row.get("task_uid");
            let task_uid = Uuid::parse_str(&task_uid_str).map_err(|e| {
                crate::domain::errors::AppError::internal(
                    "DATABASE_ROW_INVALID",
                    "invalid task_uid in sla query",
                    serde_json::json!({ "error": e.to_string() }),
                )
            })?;
            result.push(SlaTaskRow {
                id: row.get("id"),
                task_uid,
                title: row.get("title"),
                deadline: row.get("deadline"),
                created_at: row.get("created_at"),
                assigned_to_user_id: row.get("assigned_to_user_id"),
                current_sla_state: row.get("sla_state"),
                sla_last_level: row.get::<Option<i32>, _>("sla_last_level").unwrap_or(0),
            });
        }
        Ok(result)
    }

    async fn update_sla_state(
        &self,
        task_id: i64,
        state: &str,
        last_level: i32,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE tasks
             SET sla_state = ?, sla_last_level = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(state)
        .bind(last_level)
        .bind(now)
        .bind(task_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn record_escalation(
        &self,
        task_id: i64,
        level: i32,
        actor: &str,
        detail: serde_json::Value,
        now: DateTime<Utc>,
    ) -> AppResult<bool> {
        let detail_str = serde_json::to_string(&detail).unwrap_or_else(|_| "{}".to_owned());
        let result = sqlx::query(
            "INSERT OR IGNORE INTO sla_escalations (task_id, level, triggered_at, actor, detail)
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(task_id)
        .bind(level)
        .bind(now)
        .bind(actor)
        .bind(&detail_str)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;

        // `rows_affected() == 1` means the row was newly created; 0 means
        // the UNIQUE(task_id, level) constraint fired and the INSERT was
        // silently ignored.
        Ok(result.rows_affected() == 1)
    }
}
