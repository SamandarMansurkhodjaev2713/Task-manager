use crate::domain::comment::CommentKind;
use crate::domain::errors::AppError;
use crate::domain::notification::{NotificationDeliveryState, NotificationType};
use crate::domain::task::{MessageType, TaskPriority, TaskStatus};
use crate::domain::user::UserRole;

pub(crate) const USER_COLUMNS: &str =
    "id, telegram_id, last_chat_id, telegram_username, full_name, is_employee, role, created_at, updated_at";
pub(crate) const EMPLOYEE_COLUMNS: &str =
    "id, full_name, telegram_username, email, phone, department, is_active, synced_at, created_at, updated_at";
pub(crate) const TASK_COLUMNS: &str =
    "id, task_uid, version, source_message_key, created_by_user_id, assigned_to_user_id, assigned_to_employee_id, title, description, acceptance_criteria, expected_result, deadline, deadline_raw, original_message, message_type, ai_model_used, ai_response_raw, status, priority, blocked_reason, telegram_chat_id, telegram_message_id, telegram_task_message_id, tags, created_at, sent_at, started_at, blocked_at, review_requested_at, completed_at, cancelled_at, updated_at";
pub(crate) const NOTIFICATION_COLUMNS: &str =
    "id, task_id, recipient_user_id, notification_type, message, dedupe_key, telegram_message_id, delivery_state, is_sent, is_read, attempt_count, sent_at, read_at, next_attempt_at, last_error_code, created_at";
pub(crate) const AUDIT_COLUMNS: &str =
    "id, task_id, action, old_status, new_status, changed_by_user_id, metadata, created_at";
pub(crate) const COMMENT_COLUMNS: &str = "id, task_id, author_user_id, kind, body, created_at";

pub(crate) fn bool_as_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

pub(crate) fn user_role_to_db(value: UserRole) -> &'static str {
    match value {
        UserRole::User => "user",
        UserRole::Manager => "manager",
        UserRole::Admin => "admin",
    }
}

pub(crate) fn task_status_to_db(value: TaskStatus) -> &'static str {
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

pub(crate) fn task_priority_to_db(value: TaskPriority) -> &'static str {
    match value {
        TaskPriority::Low => "low",
        TaskPriority::Medium => "medium",
        TaskPriority::High => "high",
        TaskPriority::Urgent => "urgent",
    }
}

pub(crate) fn message_type_to_db(value: MessageType) -> &'static str {
    match value {
        MessageType::Text => "text",
        MessageType::Voice => "voice",
    }
}

pub(crate) fn notification_type_to_db(value: NotificationType) -> &'static str {
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

pub(crate) fn delivery_state_to_db(value: NotificationDeliveryState) -> &'static str {
    match value {
        NotificationDeliveryState::Pending => "pending",
        NotificationDeliveryState::Sent => "sent",
        NotificationDeliveryState::RetryPending => "retry_pending",
        NotificationDeliveryState::Failed => "failed",
    }
}

pub(crate) fn audit_action_to_db(value: crate::domain::audit::AuditAction) -> &'static str {
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

pub(crate) fn comment_kind_to_db(value: CommentKind) -> &'static str {
    match value {
        CommentKind::Context => "context",
        CommentKind::Blocker => "blocker",
        CommentKind::System => "system",
    }
}

pub(crate) fn database_error(error: sqlx::Error) -> AppError {
    AppError::internal(
        "DATABASE_OPERATION_FAILED",
        "SQLite operation failed",
        serde_json::json!({ "error": error.to_string() }),
    )
}

pub(crate) fn serialization_error(error: serde_json::Error) -> AppError {
    AppError::internal(
        "JSON_SERIALIZATION_FAILED",
        "Failed to serialize JSON payload",
        serde_json::json!({ "error": error.to_string() }),
    )
}
