use chrono::NaiveDate;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use uuid::Uuid;

use crate::application::ports::repositories::{PersistedTask, TaskRepository};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::task::{Task, TaskStats};
use crate::infrastructure::db::models::TaskRow;

use super::common::{
    database_error, message_type_to_db, serialization_error, task_priority_to_db,
    task_status_to_db, TASK_COLUMNS,
};

#[derive(Clone)]
pub struct SqliteTaskRepository {
    pool: SqlitePool,
}

impl SqliteTaskRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn find_by_source_message_key(
        &self,
        source_message_key: &str,
    ) -> AppResult<Option<Task>> {
        let query = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE source_message_key = ?");
        let row = sqlx::query_as::<_, TaskRow>(&query)
            .bind(source_message_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    fn list_query(
        scope_column: &'static str,
        user_id: i64,
        cursor: Option<String>,
        limit: u32,
    ) -> QueryBuilder<'static, Sqlite> {
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "SELECT {TASK_COLUMNS} FROM tasks WHERE {scope_column} = "
        ));
        builder.push_bind(user_id);

        if let Some(cursor_value) = cursor {
            builder.push(" AND task_uid < ");
            builder.push_bind(cursor_value);
        }

        builder.push(" ORDER BY task_uid DESC LIMIT ");
        builder.push_bind(i64::from(limit));
        builder
    }

    fn list_all_query(cursor: Option<String>, limit: u32) -> QueryBuilder<'static, Sqlite> {
        let mut builder =
            QueryBuilder::<Sqlite>::new(format!("SELECT {TASK_COLUMNS} FROM tasks WHERE 1 = 1"));

        if let Some(cursor_value) = cursor {
            builder.push(" AND task_uid < ");
            builder.push_bind(cursor_value);
        }

        builder.push(" ORDER BY task_uid DESC LIMIT ");
        builder.push_bind(i64::from(limit));
        builder
    }
}

#[async_trait::async_trait]
impl TaskRepository for SqliteTaskRepository {
    async fn create_if_absent(&self, task: &Task) -> AppResult<PersistedTask> {
        let acceptance_criteria =
            serde_json::to_string(&task.acceptance_criteria).map_err(serialization_error)?;
        let tags = serde_json::to_string(&task.tags).map_err(serialization_error)?;

        let insert_result = sqlx::query(
            "INSERT OR IGNORE INTO tasks (
                task_uid, version, source_message_key, created_by_user_id, assigned_to_user_id, assigned_to_employee_id,
                title, description, acceptance_criteria, expected_result, deadline, deadline_raw, original_message,
                message_type, ai_model_used, ai_response_raw, status, priority, blocked_reason, telegram_chat_id, telegram_message_id,
                telegram_task_message_id, tags, created_at, sent_at, started_at, blocked_at, review_requested_at, completed_at, cancelled_at, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        )
        .bind(task.task_uid.to_string())
        .bind(task.version)
        .bind(&task.source_message_key)
        .bind(task.created_by_user_id)
        .bind(task.assigned_to_user_id)
        .bind(task.assigned_to_employee_id)
        .bind(&task.title)
        .bind(&task.description)
        .bind(acceptance_criteria)
        .bind(&task.expected_result)
        .bind(task.deadline)
        .bind(&task.deadline_raw)
        .bind(&task.original_message)
        .bind(message_type_to_db(task.message_type))
        .bind(&task.ai_model_used)
        .bind(&task.ai_response_raw)
        .bind(task_status_to_db(task.status))
        .bind(task_priority_to_db(task.priority))
        .bind(&task.blocked_reason)
        .bind(task.telegram_chat_id)
        .bind(task.telegram_message_id)
        .bind(task.telegram_task_message_id)
        .bind(tags)
        .bind(task.created_at)
        .bind(task.sent_at)
        .bind(task.started_at)
        .bind(task.blocked_at)
        .bind(task.review_requested_at)
        .bind(task.completed_at)
        .bind(task.cancelled_at)
        .bind(task.updated_at)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;

        let inserted = insert_result.rows_affected() > 0;
        let stored_task = self
            .find_by_source_message_key(&task.source_message_key)
            .await?
            .ok_or_else(|| {
                AppError::internal(
                    "TASK_PERSISTENCE_FAILED",
                    "Task insert completed without a readable record",
                    serde_json::json!({ "task_uid": task.task_uid }),
                )
            })?;

        Ok(if inserted {
            PersistedTask::Created(stored_task)
        } else {
            PersistedTask::Existing(stored_task)
        })
    }

    async fn find_by_uid(&self, task_uid: Uuid) -> AppResult<Option<Task>> {
        let query = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE task_uid = ?");
        let row = sqlx::query_as::<_, TaskRow>(&query)
            .bind(task_uid.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn find_by_id(&self, task_id: i64) -> AppResult<Option<Task>> {
        let query = format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?");
        let row = sqlx::query_as::<_, TaskRow>(&query)
            .bind(task_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn update(&self, task: &Task) -> AppResult<Task> {
        let query = format!(
            "UPDATE tasks SET
               assigned_to_user_id = ?, assigned_to_employee_id = ?, title = ?, description = ?,
               acceptance_criteria = ?, expected_result = ?, deadline = ?, deadline_raw = ?,
               status = ?, priority = ?, blocked_reason = ?, telegram_task_message_id = ?, sent_at = ?, started_at = ?,
               blocked_at = ?, review_requested_at = ?, completed_at = ?, cancelled_at = ?, updated_at = ?, version = ?
             WHERE task_uid = ? AND version = ?
             RETURNING {TASK_COLUMNS}"
        );
        let row = sqlx::query_as::<_, TaskRow>(&query)
            .bind(task.assigned_to_user_id)
            .bind(task.assigned_to_employee_id)
            .bind(&task.title)
            .bind(&task.description)
            .bind(serde_json::to_string(&task.acceptance_criteria).map_err(serialization_error)?)
            .bind(&task.expected_result)
            .bind(task.deadline)
            .bind(&task.deadline_raw)
            .bind(task_status_to_db(task.status))
            .bind(task_priority_to_db(task.priority))
            .bind(&task.blocked_reason)
            .bind(task.telegram_task_message_id)
            .bind(task.sent_at)
            .bind(task.started_at)
            .bind(task.blocked_at)
            .bind(task.review_requested_at)
            .bind(task.completed_at)
            .bind(task.cancelled_at)
            .bind(task.updated_at)
            .bind(task.version)
            .bind(task.task_uid.to_string())
            .bind(task.version - 1)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?
            .ok_or_else(|| {
                AppError::conflict(
                    "TASK_VERSION_CONFLICT",
                    "Task was updated concurrently, please reopen the card and try again",
                    serde_json::json!({
                        "task_uid": task.task_uid,
                        "expected_previous_version": task.version - 1,
                    }),
                )
            })?;
        row.try_into()
    }

    async fn list_assigned_to_user(
        &self,
        user_id: i64,
        cursor: Option<String>,
        limit: u32,
    ) -> AppResult<Vec<Task>> {
        let mut query = Self::list_query("assigned_to_user_id", user_id, cursor, limit);
        let rows = query
            .build_query_as::<TaskRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_open_assigned_to_employee_without_user(
        &self,
        employee_id: i64,
        limit: i64,
    ) -> AppResult<Vec<Task>> {
        let query = format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE assigned_to_employee_id = ?
               AND assigned_to_user_id IS NULL
               AND status IN ('created', 'sent', 'in_progress', 'blocked', 'in_review')
             ORDER BY updated_at DESC, task_uid DESC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, TaskRow>(&query)
            .bind(employee_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_created_by_user(
        &self,
        user_id: i64,
        cursor: Option<String>,
        limit: u32,
    ) -> AppResult<Vec<Task>> {
        let mut query = Self::list_query("created_by_user_id", user_id, cursor, limit);
        let rows = query
            .build_query_as::<TaskRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_all(&self, cursor: Option<String>, limit: u32) -> AppResult<Vec<Task>> {
        let mut query = Self::list_all_query(cursor, limit);
        let rows = query
            .build_query_as::<TaskRow>()
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn get_due_between(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        limit: i64,
    ) -> AppResult<Vec<Task>> {
        let query = format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE deadline >= ? AND deadline <= ? AND status IN ('created', 'sent', 'in_progress', 'blocked', 'in_review')
             ORDER BY deadline ASC, task_uid ASC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, TaskRow>(&query)
            .bind(start)
            .bind(end)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn get_overdue(&self, as_of: NaiveDate, limit: i64) -> AppResult<Vec<Task>> {
        let query = format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE deadline < ? AND status IN ('created', 'sent', 'in_progress', 'blocked', 'in_review')
             ORDER BY deadline ASC, task_uid ASC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, TaskRow>(&query)
            .bind(as_of)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn count_stats_for_user(&self, user_id: i64) -> AppResult<TaskStats> {
        let row = sqlx::query(
            "SELECT
                COUNT(*) AS created_count,
                COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0) AS completed_count,
                COALESCE(SUM(CASE WHEN status IN ('created', 'sent', 'in_progress', 'blocked', 'in_review') THEN 1 ELSE 0 END), 0) AS active_count,
                COALESCE(SUM(CASE WHEN deadline IS NOT NULL AND deadline < date('now') AND status NOT IN ('completed', 'cancelled') THEN 1 ELSE 0 END), 0) AS overdue_count,
                AVG(CASE WHEN completed_at IS NOT NULL THEN (julianday(completed_at) - julianday(created_at)) * 24 ELSE NULL END) AS average_completion_hours
             FROM tasks
             WHERE created_by_user_id = ?"
        )
        .bind(user_id)
        .fetch_one(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(TaskStats {
            created_count: row.get::<i64, _>("created_count"),
            completed_count: row.get::<i64, _>("completed_count"),
            active_count: row.get::<i64, _>("active_count"),
            overdue_count: row.get::<i64, _>("overdue_count"),
            average_completion_hours: row
                .get::<Option<f64>, _>("average_completion_hours")
                .map(|value| value.round() as i64),
        })
    }

    async fn count_stats_global(&self) -> AppResult<TaskStats> {
        let row = sqlx::query(
            "SELECT
                COUNT(*) AS created_count,
                COALESCE(SUM(CASE WHEN status = 'completed' THEN 1 ELSE 0 END), 0) AS completed_count,
                COALESCE(SUM(CASE WHEN status IN ('created', 'sent', 'in_progress', 'blocked', 'in_review') THEN 1 ELSE 0 END), 0) AS active_count,
                COALESCE(SUM(CASE WHEN deadline IS NOT NULL AND deadline < date('now') AND status NOT IN ('completed', 'cancelled') THEN 1 ELSE 0 END), 0) AS overdue_count,
                AVG(CASE WHEN completed_at IS NOT NULL THEN (julianday(completed_at) - julianday(created_at)) * 24 ELSE NULL END) AS average_completion_hours
             FROM tasks"
        )
        .fetch_one(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(TaskStats {
            created_count: row.get::<i64, _>("created_count"),
            completed_count: row.get::<i64, _>("completed_count"),
            active_count: row.get::<i64, _>("active_count"),
            overdue_count: row.get::<i64, _>("overdue_count"),
            average_completion_hours: row
                .get::<Option<f64>, _>("average_completion_hours")
                .map(|value| value.round() as i64),
        })
    }

    async fn list_open(&self, limit: i64) -> AppResult<Vec<Task>> {
        let query = format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE status IN ('created', 'sent', 'in_progress', 'blocked', 'in_review')
             ORDER BY updated_at DESC, task_uid DESC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, TaskRow>(&query)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}
