use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::FromRow;
use uuid::Uuid;

use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::comment::{CommentKind, TaskComment};
use crate::domain::employee::Employee;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::task::{MessageType, Task, TaskPriority, TaskStatus};
use crate::domain::user::{User, UserRole};

#[derive(Debug, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub telegram_id: i64,
    pub last_chat_id: Option<i64>,
    pub telegram_username: Option<String>,
    pub full_name: Option<String>,
    pub linked_employee_id: Option<i64>,
    pub is_employee: i64,
    pub role: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct EmployeeRow {
    pub id: i64,
    pub full_name: String,
    pub telegram_username: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub is_active: i64,
    pub synced_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct TaskRow {
    pub id: i64,
    pub task_uid: String,
    pub version: i64,
    pub source_message_key: String,
    pub created_by_user_id: i64,
    pub assigned_to_user_id: Option<i64>,
    pub assigned_to_employee_id: Option<i64>,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: String,
    pub expected_result: String,
    pub deadline: Option<NaiveDate>,
    pub deadline_raw: Option<String>,
    pub original_message: String,
    pub message_type: String,
    pub ai_model_used: String,
    pub ai_response_raw: String,
    pub status: String,
    pub priority: String,
    pub blocked_reason: Option<String>,
    pub telegram_chat_id: i64,
    pub telegram_message_id: i32,
    pub telegram_task_message_id: Option<i32>,
    pub tags: String,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub blocked_at: Option<DateTime<Utc>>,
    pub review_requested_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct NotificationRow {
    pub id: i64,
    pub task_id: Option<i64>,
    pub recipient_user_id: i64,
    pub notification_type: String,
    pub message: String,
    pub dedupe_key: String,
    pub telegram_message_id: Option<i32>,
    pub delivery_state: String,
    pub is_sent: i64,
    pub is_read: i64,
    pub attempt_count: i32,
    pub sent_at: Option<DateTime<Utc>>,
    pub read_at: Option<DateTime<Utc>>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct AuditLogRow {
    pub id: i64,
    pub task_id: i64,
    pub action: String,
    pub old_status: Option<String>,
    pub new_status: Option<String>,
    pub changed_by_user_id: Option<i64>,
    pub metadata: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, FromRow)]
pub struct CommentRow {
    pub id: i64,
    pub task_id: i64,
    pub author_user_id: i64,
    pub kind: String,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

impl TryFrom<UserRow> for User {
    type Error = AppError;

    fn try_from(value: UserRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: Some(value.id),
            telegram_id: value.telegram_id,
            last_chat_id: value.last_chat_id,
            telegram_username: value.telegram_username,
            full_name: value.full_name,
            linked_employee_id: value.linked_employee_id,
            is_employee: value.is_employee != 0,
            role: parse_user_role(&value.role)?,
            created_at: value.created_at,
            updated_at: value.updated_at,
        })
    }
}

impl From<EmployeeRow> for Employee {
    fn from(value: EmployeeRow) -> Self {
        Self {
            id: Some(value.id),
            full_name: value.full_name,
            telegram_username: value.telegram_username,
            email: value.email,
            phone: value.phone,
            department: value.department,
            is_active: value.is_active != 0,
            synced_at: value.synced_at,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}

impl TryFrom<TaskRow> for Task {
    type Error = AppError;

    fn try_from(value: TaskRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: Some(value.id),
            task_uid: Uuid::parse_str(&value.task_uid)
                .map_err(|error| invalid_row("task_uid", error.to_string()))?,
            version: value.version,
            source_message_key: value.source_message_key,
            created_by_user_id: value.created_by_user_id,
            assigned_to_user_id: value.assigned_to_user_id,
            assigned_to_employee_id: value.assigned_to_employee_id,
            title: value.title,
            description: value.description,
            acceptance_criteria: parse_json_array(
                &value.acceptance_criteria,
                "acceptance_criteria",
            )?,
            expected_result: value.expected_result,
            deadline: value.deadline,
            deadline_raw: value.deadline_raw,
            original_message: value.original_message,
            message_type: parse_message_type(&value.message_type)?,
            ai_model_used: value.ai_model_used,
            ai_response_raw: value.ai_response_raw,
            status: parse_task_status(&value.status)?,
            priority: parse_task_priority(&value.priority)?,
            blocked_reason: value.blocked_reason,
            telegram_chat_id: value.telegram_chat_id,
            telegram_message_id: value.telegram_message_id,
            telegram_task_message_id: value.telegram_task_message_id,
            tags: parse_json_array(&value.tags, "tags")?,
            created_at: value.created_at,
            sent_at: value.sent_at,
            started_at: value.started_at,
            blocked_at: value.blocked_at,
            review_requested_at: value.review_requested_at,
            completed_at: value.completed_at,
            cancelled_at: value.cancelled_at,
            updated_at: value.updated_at,
        })
    }
}

impl TryFrom<NotificationRow> for Notification {
    type Error = AppError;

    fn try_from(value: NotificationRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: Some(value.id),
            task_id: value.task_id,
            recipient_user_id: value.recipient_user_id,
            notification_type: parse_notification_type(&value.notification_type)?,
            message: value.message,
            dedupe_key: value.dedupe_key,
            telegram_message_id: value.telegram_message_id,
            delivery_state: parse_delivery_state(&value.delivery_state)?,
            is_sent: value.is_sent != 0,
            is_read: value.is_read != 0,
            attempt_count: value.attempt_count,
            sent_at: value.sent_at,
            read_at: value.read_at,
            next_attempt_at: value.next_attempt_at,
            last_error_code: value.last_error_code,
            created_at: value.created_at,
        })
    }
}

impl TryFrom<AuditLogRow> for AuditLogEntry {
    type Error = AppError;

    fn try_from(value: AuditLogRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: Some(value.id),
            task_id: value.task_id,
            action: parse_audit_action(&value.action)?,
            old_status: value.old_status,
            new_status: value.new_status,
            changed_by_user_id: value.changed_by_user_id,
            metadata: serde_json::from_str::<Value>(&value.metadata)
                .map_err(|error| invalid_row("metadata", error.to_string()))?,
            created_at: value.created_at,
        })
    }
}

impl TryFrom<CommentRow> for TaskComment {
    type Error = AppError;

    fn try_from(value: CommentRow) -> Result<Self, Self::Error> {
        Ok(Self {
            id: Some(value.id),
            task_id: value.task_id,
            author_user_id: value.author_user_id,
            kind: parse_comment_kind(&value.kind)?,
            body: value.body,
            created_at: value.created_at,
        })
    }
}

fn parse_json_array(payload: &str, field: &'static str) -> AppResult<Vec<String>> {
    serde_json::from_str::<Vec<String>>(payload)
        .map_err(|error| invalid_row(field, error.to_string()))
}

fn parse_user_role(value: &str) -> AppResult<UserRole> {
    match value {
        "user" => Ok(UserRole::User),
        "manager" => Ok(UserRole::Manager),
        "admin" => Ok(UserRole::Admin),
        _ => Err(invalid_row("role", value)),
    }
}

fn parse_task_status(value: &str) -> AppResult<TaskStatus> {
    match value {
        "created" => Ok(TaskStatus::Created),
        "sent" => Ok(TaskStatus::Sent),
        "in_progress" => Ok(TaskStatus::InProgress),
        "blocked" => Ok(TaskStatus::Blocked),
        "in_review" => Ok(TaskStatus::InReview),
        "completed" => Ok(TaskStatus::Completed),
        "cancelled" => Ok(TaskStatus::Cancelled),
        _ => Err(invalid_row("status", value)),
    }
}

fn parse_task_priority(value: &str) -> AppResult<TaskPriority> {
    match value {
        "low" => Ok(TaskPriority::Low),
        "medium" => Ok(TaskPriority::Medium),
        "high" => Ok(TaskPriority::High),
        "urgent" => Ok(TaskPriority::Urgent),
        _ => Err(invalid_row("priority", value)),
    }
}

fn parse_message_type(value: &str) -> AppResult<MessageType> {
    match value {
        "text" => Ok(MessageType::Text),
        "voice" => Ok(MessageType::Voice),
        _ => Err(invalid_row("message_type", value)),
    }
}

fn parse_notification_type(value: &str) -> AppResult<NotificationType> {
    match value {
        "task_assigned" => Ok(NotificationType::TaskAssigned),
        "task_updated" => Ok(NotificationType::TaskUpdated),
        "deadline_reminder" => Ok(NotificationType::DeadlineReminder),
        "task_completed" => Ok(NotificationType::TaskCompleted),
        "task_cancelled" => Ok(NotificationType::TaskCancelled),
        "task_review_requested" => Ok(NotificationType::TaskReviewRequested),
        "task_blocked" => Ok(NotificationType::TaskBlocked),
        "daily_summary" => Ok(NotificationType::DailySummary),
        _ => Err(invalid_row("notification_type", value)),
    }
}

fn parse_delivery_state(value: &str) -> AppResult<NotificationDeliveryState> {
    match value {
        "pending" => Ok(NotificationDeliveryState::Pending),
        "sent" => Ok(NotificationDeliveryState::Sent),
        "retry_pending" => Ok(NotificationDeliveryState::RetryPending),
        "failed" => Ok(NotificationDeliveryState::Failed),
        _ => Err(invalid_row("delivery_state", value)),
    }
}

fn parse_audit_action(value: &str) -> AppResult<AuditAction> {
    match value {
        "created" => Ok(AuditAction::Created),
        "sent" => Ok(AuditAction::Sent),
        "assigned" => Ok(AuditAction::Assigned),
        "status_changed" => Ok(AuditAction::StatusChanged),
        "review_requested" => Ok(AuditAction::ReviewRequested),
        "reassigned" => Ok(AuditAction::Reassigned),
        "blocked" => Ok(AuditAction::Blocked),
        "commented" => Ok(AuditAction::Commented),
        "edited" => Ok(AuditAction::Edited),
        "cancelled" => Ok(AuditAction::Cancelled),
        "employees_synced" => Ok(AuditAction::EmployeesSynced),
        _ => Err(invalid_row("audit_action", value)),
    }
}

fn parse_comment_kind(value: &str) -> AppResult<CommentKind> {
    match value {
        "context" => Ok(CommentKind::Context),
        "blocker" => Ok(CommentKind::Blocker),
        "system" => Ok(CommentKind::System),
        _ => Err(invalid_row("comment_kind", value)),
    }
}

fn invalid_row(field: &'static str, raw: impl ToString) -> AppError {
    AppError::internal(
        "DATABASE_ROW_INVALID",
        "Database row contains an invalid value",
        serde_json::json!({ "field": field, "raw": raw.to_string() }),
    )
}
