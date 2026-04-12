use chrono::{DateTime, NaiveDate, Utc};
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use uuid::Uuid;

use crate::application::ports::repositories::{
    AuditLogRepository, CommentRepository, EmployeeRepository, NotificationRepository,
    PersistedTask, TaskRepository, UserRepository,
};
use crate::domain::audit::AuditLogEntry;
use crate::domain::comment::{CommentKind, TaskComment};
use crate::domain::employee::Employee;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::task::{MessageType, Task, TaskPriority, TaskStats, TaskStatus};
use crate::domain::user::{User, UserRole};
use crate::infrastructure::db::models::{
    AuditLogRow, CommentRow, EmployeeRow, NotificationRow, TaskRow, UserRow,
};

const USER_COLUMNS: &str = "id, telegram_id, last_chat_id, telegram_username, full_name, is_employee, role, created_at, updated_at";
const EMPLOYEE_COLUMNS: &str = "id, full_name, telegram_username, email, phone, department, is_active, synced_at, created_at, updated_at";
const TASK_COLUMNS: &str = "id, task_uid, version, source_message_key, created_by_user_id, assigned_to_user_id, assigned_to_employee_id, title, description, acceptance_criteria, expected_result, deadline, deadline_raw, original_message, message_type, ai_model_used, ai_response_raw, status, priority, blocked_reason, telegram_chat_id, telegram_message_id, telegram_task_message_id, tags, created_at, sent_at, started_at, blocked_at, review_requested_at, completed_at, cancelled_at, updated_at";
const NOTIFICATION_COLUMNS: &str = "id, task_id, recipient_user_id, notification_type, message, dedupe_key, telegram_message_id, delivery_state, is_sent, is_read, attempt_count, sent_at, read_at, next_attempt_at, last_error_code, created_at";
const AUDIT_COLUMNS: &str =
    "id, task_id, action, old_status, new_status, changed_by_user_id, metadata, created_at";
const COMMENT_COLUMNS: &str = "id, task_id, author_user_id, kind, body, created_at";

#[derive(Clone)]
pub struct SqliteUserRepository {
    pool: SqlitePool,
}

#[derive(Clone)]
pub struct SqliteEmployeeRepository {
    pool: SqlitePool,
}

#[derive(Clone)]
pub struct SqliteTaskRepository {
    pool: SqlitePool,
}

#[derive(Clone)]
pub struct SqliteNotificationRepository {
    pool: SqlitePool,
}

#[derive(Clone)]
pub struct SqliteAuditLogRepository {
    pool: SqlitePool,
}

#[derive(Clone)]
pub struct SqliteCommentRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl SqliteEmployeeRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
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

impl SqliteNotificationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl SqliteAuditLogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

impl SqliteCommentRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl UserRepository for SqliteUserRepository {
    async fn upsert_from_message(&self, user: &User) -> AppResult<User> {
        let query = format!(
            "INSERT INTO users (telegram_id, last_chat_id, telegram_username, full_name, is_employee, role, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(telegram_id) DO UPDATE SET
               last_chat_id = excluded.last_chat_id,
               telegram_username = excluded.telegram_username,
               full_name = excluded.full_name,
               is_employee = MAX(users.is_employee, excluded.is_employee),
               updated_at = excluded.updated_at
             RETURNING {USER_COLUMNS}"
        );

        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(user.telegram_id)
            .bind(user.last_chat_id)
            .bind(&user.telegram_username)
            .bind(&user.full_name)
            .bind(bool_as_i64(user.is_employee))
            .bind(user_role_to_db(user.role))
            .bind(user.created_at)
            .bind(user.updated_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn find_by_id(&self, user_id: i64) -> AppResult<Option<User>> {
        let query = format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?");
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn find_by_telegram_id(&self, telegram_id: i64) -> AppResult<Option<User>> {
        let query = format!("SELECT {USER_COLUMNS} FROM users WHERE telegram_id = ?");
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(telegram_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn find_by_username(&self, username: &str) -> AppResult<Option<User>> {
        let query =
            format!("SELECT {USER_COLUMNS} FROM users WHERE lower(telegram_username) = lower(?)");
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(username.trim_start_matches('@'))
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn list_with_chat_id(&self) -> AppResult<Vec<User>> {
        let query = format!(
            "SELECT {USER_COLUMNS} FROM users WHERE last_chat_id IS NOT NULL ORDER BY id ASC"
        );
        let rows = sqlx::query_as::<_, UserRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}

#[async_trait::async_trait]
impl EmployeeRepository for SqliteEmployeeRepository {
    async fn upsert_many(&self, employees: &[Employee]) -> AppResult<usize> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;

        for employee in employees {
            sqlx::query(
                "INSERT INTO employees (full_name, telegram_username, email, phone, department, is_active, synced_at, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(full_name) DO UPDATE SET
                   telegram_username = excluded.telegram_username,
                   email = excluded.email,
                   phone = excluded.phone,
                   department = excluded.department,
                   is_active = excluded.is_active,
                   synced_at = excluded.synced_at,
                   updated_at = excluded.updated_at",
            )
            .bind(&employee.full_name)
            .bind(employee.telegram_username.as_deref())
            .bind(employee.email.as_deref())
            .bind(employee.phone.as_deref())
            .bind(employee.department.as_deref())
            .bind(bool_as_i64(employee.is_active))
            .bind(employee.synced_at)
            .bind(employee.created_at)
            .bind(employee.updated_at)
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        }

        transaction.commit().await.map_err(database_error)?;
        Ok(employees.len())
    }

    async fn list_active(&self) -> AppResult<Vec<Employee>> {
        let query = format!(
            "SELECT {EMPLOYEE_COLUMNS} FROM employees WHERE is_active = 1 ORDER BY full_name ASC"
        );
        let rows = sqlx::query_as::<_, EmployeeRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_by_id(&self, employee_id: i64) -> AppResult<Option<Employee>> {
        let query = format!("SELECT {EMPLOYEE_COLUMNS} FROM employees WHERE id = ?");
        let row = sqlx::query_as::<_, EmployeeRow>(&query)
            .bind(employee_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(row.map(Into::into))
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
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
                    serde_json::json!({ "task_uid": task.task_uid, "expected_previous_version": task.version - 1 }),
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

#[async_trait::async_trait]
impl NotificationRepository for SqliteNotificationRepository {
    async fn enqueue(&self, notification: &Notification) -> AppResult<Notification> {
        let query = format!(
            "INSERT OR IGNORE INTO notifications (
                task_id, recipient_user_id, notification_type, message, dedupe_key, telegram_message_id,
                delivery_state, is_sent, is_read, attempt_count, sent_at, read_at, next_attempt_at, last_error_code, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        );
        sqlx::query(&query)
            .bind(notification.task_id)
            .bind(notification.recipient_user_id)
            .bind(notification_type_to_db(notification.notification_type))
            .bind(&notification.message)
            .bind(&notification.dedupe_key)
            .bind(notification.telegram_message_id)
            .bind(delivery_state_to_db(notification.delivery_state))
            .bind(bool_as_i64(notification.is_sent))
            .bind(bool_as_i64(notification.is_read))
            .bind(notification.attempt_count)
            .bind(notification.sent_at)
            .bind(notification.read_at)
            .bind(notification.next_attempt_at)
            .bind(&notification.last_error_code)
            .bind(notification.created_at)
            .execute(&self.pool)
            .await
            .map_err(database_error)?;

        let lookup =
            format!("SELECT {NOTIFICATION_COLUMNS} FROM notifications WHERE dedupe_key = ?");
        let row = sqlx::query_as::<_, NotificationRow>(&lookup)
            .bind(&notification.dedupe_key)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_pending(&self, limit: i64) -> AppResult<Vec<Notification>> {
        let query = format!(
            "SELECT {NOTIFICATION_COLUMNS} FROM notifications
             WHERE delivery_state IN ('pending', 'retry_pending')
               AND (next_attempt_at IS NULL OR next_attempt_at <= ?)
             ORDER BY created_at ASC, id ASC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, NotificationRow>(&query)
            .bind(Utc::now())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn mark_sent(
        &self,
        notification_id: i64,
        telegram_message_id: i32,
        sent_at_utc: DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE notifications
             SET is_sent = 1, delivery_state = 'sent', telegram_message_id = ?, sent_at = ?, next_attempt_at = NULL, last_error_code = NULL
             WHERE id = ?",
        )
        .bind(telegram_message_id)
        .bind(sent_at_utc)
        .bind(notification_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn mark_retry_pending(
        &self,
        notification_id: i64,
        next_attempt_at: DateTime<Utc>,
        error_code: &'static str,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE notifications
             SET attempt_count = attempt_count + 1,
                 delivery_state = 'retry_pending',
                 next_attempt_at = ?,
                 last_error_code = ?
             WHERE id = ?",
        )
        .bind(next_attempt_at)
        .bind(error_code)
        .bind(notification_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn mark_failed(&self, notification_id: i64, error_code: &'static str) -> AppResult<()> {
        sqlx::query(
            "UPDATE notifications
             SET attempt_count = attempt_count + 1,
                 delivery_state = 'failed',
                 next_attempt_at = NULL,
                 last_error_code = ?
             WHERE id = ?",
        )
        .bind(error_code)
        .bind(notification_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn find_latest_for_task_and_recipient(
        &self,
        task_id: i64,
        recipient_user_id: i64,
        notification_type: NotificationType,
    ) -> AppResult<Option<Notification>> {
        let query = format!(
            "SELECT {NOTIFICATION_COLUMNS} FROM notifications
             WHERE task_id = ? AND recipient_user_id = ? AND notification_type = ?
             ORDER BY created_at DESC, id DESC
             LIMIT 1"
        );
        let row = sqlx::query_as::<_, NotificationRow>(&query)
            .bind(task_id)
            .bind(recipient_user_id)
            .bind(notification_type_to_db(notification_type))
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }
}

#[async_trait::async_trait]
impl AuditLogRepository for SqliteAuditLogRepository {
    async fn append(&self, entry: &AuditLogEntry) -> AppResult<AuditLogEntry> {
        let query = format!(
            "INSERT INTO task_history (task_id, action, old_status, new_status, changed_by_user_id, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             RETURNING {AUDIT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, AuditLogRow>(&query)
            .bind(entry.task_id)
            .bind(audit_action_to_db(entry.action))
            .bind(&entry.old_status)
            .bind(&entry.new_status)
            .bind(entry.changed_by_user_id)
            .bind(serde_json::to_string(&entry.metadata).map_err(serialization_error)?)
            .bind(entry.created_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_for_task(&self, task_id: i64) -> AppResult<Vec<AuditLogEntry>> {
        let query = format!(
            "SELECT {AUDIT_COLUMNS} FROM task_history
             WHERE task_id = ?
             ORDER BY created_at DESC, id DESC"
        );
        let rows = sqlx::query_as::<_, AuditLogRow>(&query)
            .bind(task_id)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}

#[async_trait::async_trait]
impl CommentRepository for SqliteCommentRepository {
    async fn create(&self, comment: &TaskComment) -> AppResult<TaskComment> {
        let query = format!(
            "INSERT INTO comments (task_id, author_user_id, kind, body, created_at)
             VALUES (?, ?, ?, ?, ?)
             RETURNING {COMMENT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, CommentRow>(&query)
            .bind(comment.task_id)
            .bind(comment.author_user_id)
            .bind(comment_kind_to_db(comment.kind))
            .bind(&comment.body)
            .bind(comment.created_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_recent_for_task(&self, task_id: i64, limit: i64) -> AppResult<Vec<TaskComment>> {
        let query = format!(
            "SELECT {COMMENT_COLUMNS} FROM comments
             WHERE task_id = ?
             ORDER BY created_at DESC, id DESC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, CommentRow>(&query)
            .bind(task_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}

fn bool_as_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn user_role_to_db(value: UserRole) -> &'static str {
    match value {
        UserRole::User => "user",
        UserRole::Manager => "manager",
        UserRole::Admin => "admin",
    }
}

fn task_status_to_db(value: TaskStatus) -> &'static str {
    match value {
        TaskStatus::Created => "created",
        TaskStatus::Sent => "sent",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::InReview => "in_review",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn task_priority_to_db(value: TaskPriority) -> &'static str {
    match value {
        TaskPriority::Low => "low",
        TaskPriority::Medium => "medium",
        TaskPriority::High => "high",
        TaskPriority::Urgent => "urgent",
    }
}

fn message_type_to_db(value: MessageType) -> &'static str {
    match value {
        MessageType::Text => "text",
        MessageType::Voice => "voice",
    }
}

fn notification_type_to_db(value: NotificationType) -> &'static str {
    match value {
        NotificationType::TaskAssigned => "task_assigned",
        NotificationType::TaskUpdated => "task_updated",
        NotificationType::DeadlineReminder => "deadline_reminder",
        NotificationType::TaskCompleted => "task_completed",
        NotificationType::TaskCancelled => "task_cancelled",
        NotificationType::TaskReviewRequested => "task_review_requested",
        NotificationType::TaskBlocked => "task_blocked",
        NotificationType::DailySummary => "daily_summary",
    }
}

fn delivery_state_to_db(value: NotificationDeliveryState) -> &'static str {
    match value {
        NotificationDeliveryState::Pending => "pending",
        NotificationDeliveryState::Sent => "sent",
        NotificationDeliveryState::RetryPending => "retry_pending",
        NotificationDeliveryState::Failed => "failed",
    }
}

fn audit_action_to_db(value: crate::domain::audit::AuditAction) -> &'static str {
    match value {
        crate::domain::audit::AuditAction::Created => "created",
        crate::domain::audit::AuditAction::Sent => "sent",
        crate::domain::audit::AuditAction::Assigned => "assigned",
        crate::domain::audit::AuditAction::StatusChanged => "status_changed",
        crate::domain::audit::AuditAction::ReviewRequested => "review_requested",
        crate::domain::audit::AuditAction::Reassigned => "reassigned",
        crate::domain::audit::AuditAction::Blocked => "blocked",
        crate::domain::audit::AuditAction::Commented => "commented",
        crate::domain::audit::AuditAction::Edited => "edited",
        crate::domain::audit::AuditAction::Cancelled => "cancelled",
        crate::domain::audit::AuditAction::EmployeesSynced => "employees_synced",
    }
}

fn comment_kind_to_db(value: CommentKind) -> &'static str {
    match value {
        CommentKind::Context => "context",
        CommentKind::Blocker => "blocker",
        CommentKind::System => "system",
    }
}

fn database_error(error: sqlx::Error) -> AppError {
    AppError::internal(
        "DATABASE_OPERATION_FAILED",
        "SQLite operation failed",
        serde_json::json!({ "error": error.to_string() }),
    )
}

fn serialization_error(error: serde_json::Error) -> AppError {
    AppError::internal(
        "JSON_SERIALIZATION_FAILED",
        "Failed to serialize JSON payload",
        serde_json::json!({ "error": error.to_string() }),
    )
}
