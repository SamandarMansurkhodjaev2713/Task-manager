use async_trait::async_trait;
use chrono::NaiveDate;
use uuid::Uuid;

use crate::domain::audit::AuditLogEntry;
use crate::domain::comment::TaskComment;
use crate::domain::employee::Employee;
use crate::domain::errors::AppResult;
use crate::domain::notification::{Notification, NotificationType};
use crate::domain::task::{Task, TaskStats};
use crate::domain::user::User;

#[derive(Debug, Clone)]
pub enum PersistedTask {
    Created(Task),
    Existing(Task),
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn upsert_from_message(&self, user: &User) -> AppResult<User>;
    async fn find_by_id(&self, user_id: i64) -> AppResult<Option<User>>;
    async fn find_by_telegram_id(&self, telegram_id: i64) -> AppResult<Option<User>>;
    async fn find_by_username(&self, username: &str) -> AppResult<Option<User>>;
    async fn list_with_chat_id(&self) -> AppResult<Vec<User>>;
}

#[async_trait]
pub trait EmployeeRepository: Send + Sync {
    async fn upsert_many(&self, employees: &[Employee]) -> AppResult<usize>;
    async fn list_active(&self) -> AppResult<Vec<Employee>>;
    async fn find_by_id(&self, employee_id: i64) -> AppResult<Option<Employee>>;
}

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create_if_absent(&self, task: &Task) -> AppResult<PersistedTask>;
    async fn find_by_id(&self, task_id: i64) -> AppResult<Option<Task>>;
    async fn find_by_uid(&self, task_uid: Uuid) -> AppResult<Option<Task>>;
    async fn update(&self, task: &Task) -> AppResult<Task>;
    async fn list_assigned_to_user(
        &self,
        user_id: i64,
        cursor: Option<String>,
        limit: u32,
    ) -> AppResult<Vec<Task>>;
    async fn list_open_assigned_to_employee_without_user(
        &self,
        employee_id: i64,
        limit: i64,
    ) -> AppResult<Vec<Task>>;
    async fn list_created_by_user(
        &self,
        user_id: i64,
        cursor: Option<String>,
        limit: u32,
    ) -> AppResult<Vec<Task>>;
    async fn list_all(&self, cursor: Option<String>, limit: u32) -> AppResult<Vec<Task>>;
    async fn get_due_between(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        limit: i64,
    ) -> AppResult<Vec<Task>>;
    async fn get_overdue(&self, as_of: NaiveDate, limit: i64) -> AppResult<Vec<Task>>;
    async fn count_stats_for_user(&self, user_id: i64) -> AppResult<TaskStats>;
    async fn count_stats_global(&self) -> AppResult<TaskStats>;
    async fn list_open(&self, limit: i64) -> AppResult<Vec<Task>>;
}

#[async_trait]
pub trait NotificationRepository: Send + Sync {
    async fn enqueue(&self, notification: &Notification) -> AppResult<Notification>;
    async fn list_pending(&self, limit: i64) -> AppResult<Vec<Notification>>;
    async fn mark_sent(
        &self,
        notification_id: i64,
        telegram_message_id: i32,
        sent_at_utc: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<()>;
    async fn mark_retry_pending(
        &self,
        notification_id: i64,
        next_attempt_at: chrono::DateTime<chrono::Utc>,
        error_code: &'static str,
    ) -> AppResult<()>;
    async fn mark_failed(&self, notification_id: i64, error_code: &'static str) -> AppResult<()>;
    async fn requeue(&self, notification_id: i64) -> AppResult<()>;
    async fn find_latest_for_task_and_recipient(
        &self,
        task_id: i64,
        recipient_user_id: i64,
        notification_type: NotificationType,
    ) -> AppResult<Option<Notification>>;
}

#[async_trait]
pub trait AuditLogRepository: Send + Sync {
    async fn append(&self, entry: &AuditLogEntry) -> AppResult<AuditLogEntry>;
    async fn list_for_task(&self, task_id: i64) -> AppResult<Vec<AuditLogEntry>>;
}

#[async_trait]
pub trait CommentRepository: Send + Sync {
    async fn create(&self, comment: &TaskComment) -> AppResult<TaskComment>;
    async fn list_recent_for_task(&self, task_id: i64, limit: i64) -> AppResult<Vec<TaskComment>>;
}
