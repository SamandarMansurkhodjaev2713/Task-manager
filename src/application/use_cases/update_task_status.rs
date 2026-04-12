use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::application::dto::task_views::TaskStatusSummary;
use crate::application::ports::repositories::{
    AuditLogRepository, NotificationRepository, TaskRepository,
};
use crate::application::ports::services::Clock;
use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::task::{Task, TaskStatus};
use crate::domain::user::User;

pub struct UpdateTaskStatusUseCase {
    clock: Arc<dyn Clock>,
    task_repository: Arc<dyn TaskRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
}

impl UpdateTaskStatusUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        task_repository: Arc<dyn TaskRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
        audit_log_repository: Arc<dyn AuditLogRepository>,
    ) -> Self {
        Self {
            clock,
            task_repository,
            notification_repository,
            audit_log_repository,
        }
    }

    pub async fn execute(
        &self,
        actor: &User,
        task_uid: Uuid,
        requested_status: TaskStatus,
    ) -> AppResult<TaskStatusSummary> {
        let Some(task) = self.task_repository.find_by_uid(task_uid).await? else {
            return Err(AppError::not_found(
                "TASK_NOT_FOUND",
                "Task was not found",
                json!({ "task_uid": task_uid }),
            ));
        };

        let next_status = normalize_requested_status(actor, &task, requested_status)?;
        authorize_status_change(actor, &task, next_status)?;

        let previous_status = task.status;
        let updated_task = task.transition_to(next_status, self.clock.now_utc())?;
        let saved_task = self.task_repository.update(&updated_task).await?;
        self.log_status_change(actor.id, &saved_task, previous_status, next_status)
            .await?;
        self.enqueue_participant_notifications(actor.id, &saved_task, next_status)
            .await?;

        Ok(TaskStatusSummary {
            task_uid,
            status: saved_task.status,
            message: build_status_message(previous_status, saved_task.status),
        })
    }

    async fn log_status_change(
        &self,
        actor_id: Option<i64>,
        task: &Task,
        previous_status: TaskStatus,
        next_status: TaskStatus,
    ) -> AppResult<()> {
        let Some(task_id) = task.id else {
            return Err(AppError::internal(
                "TASK_ID_MISSING",
                "Task must have a database identifier before audit logging",
                json!({ "task_uid": task.task_uid }),
            ));
        };

        let entry = AuditLogEntry {
            id: None,
            task_id,
            action: audit_action_for_status(next_status),
            old_status: Some(previous_status.to_string()),
            new_status: Some(next_status.to_string()),
            changed_by_user_id: actor_id,
            metadata: json!({}),
            created_at: self.clock.now_utc(),
        };
        let _ = self.audit_log_repository.append(&entry).await?;
        Ok(())
    }

    async fn enqueue_participant_notifications(
        &self,
        actor_id: Option<i64>,
        task: &Task,
        next_status: TaskStatus,
    ) -> AppResult<()> {
        let recipients = [Some(task.created_by_user_id), task.assigned_to_user_id];
        for recipient_user_id in recipients.into_iter().flatten() {
            if Some(recipient_user_id) == actor_id {
                continue;
            }

            let notification = Notification {
                id: None,
                task_id: task.id,
                recipient_user_id,
                notification_type: notification_type_for_status(next_status),
                message: build_participant_notification(task, next_status),
                dedupe_key: format!(
                    "task_status:{}:{}:{}:{}",
                    task.task_uid, recipient_user_id, next_status, task.version
                ),
                telegram_message_id: None,
                delivery_state: NotificationDeliveryState::Pending,
                is_sent: false,
                is_read: false,
                attempt_count: 0,
                sent_at: None,
                read_at: None,
                next_attempt_at: None,
                last_error_code: None,
                created_at: self.clock.now_utc(),
            };
            let _ = self.notification_repository.enqueue(&notification).await?;
        }

        Ok(())
    }
}

fn audit_action_for_status(next_status: TaskStatus) -> AuditAction {
    if next_status == TaskStatus::InReview {
        return AuditAction::ReviewRequested;
    }

    AuditAction::StatusChanged
}

fn notification_type_for_status(next_status: TaskStatus) -> NotificationType {
    match next_status {
        TaskStatus::Completed => NotificationType::TaskCompleted,
        TaskStatus::Cancelled => NotificationType::TaskCancelled,
        TaskStatus::InReview => NotificationType::TaskReviewRequested,
        TaskStatus::Blocked => NotificationType::TaskBlocked,
        TaskStatus::Created | TaskStatus::Sent | TaskStatus::InProgress => {
            NotificationType::TaskUpdated
        }
    }
}

fn authorize_status_change(actor: &User, task: &Task, next_status: TaskStatus) -> AppResult<()> {
    let actor_id = actor.id.ok_or_else(|| {
        AppError::unauthenticated(
            "User must be registered before changing task status",
            json!({ "telegram_id": actor.telegram_id }),
        )
    })?;

    if actor.role.is_admin() {
        return Ok(());
    }

    if is_creator(actor_id, task) && creator_can_change_status(task, next_status) {
        return Ok(());
    }

    if is_assignee(actor_id, task) && assignee_can_change_status(next_status) {
        return Ok(());
    }

    if actor.role.is_manager_or_admin() && manager_can_change_status(next_status) {
        return Ok(());
    }

    Err(AppError::unauthorized(
        "User is not allowed to change this task status",
        json!({ "task_uid": task.task_uid, "next_status": next_status }),
    ))
}

fn is_creator(actor_id: i64, task: &Task) -> bool {
    actor_id == task.created_by_user_id
}

fn is_assignee(actor_id: i64, task: &Task) -> bool {
    task.assigned_to_user_id == Some(actor_id)
}

fn creator_can_change_status(task: &Task, next_status: TaskStatus) -> bool {
    match next_status {
        TaskStatus::Completed => true,
        TaskStatus::Cancelled => true,
        TaskStatus::InProgress => task.status == TaskStatus::InReview,
        TaskStatus::Created | TaskStatus::Sent | TaskStatus::Blocked | TaskStatus::InReview => {
            false
        }
    }
}

fn assignee_can_change_status(next_status: TaskStatus) -> bool {
    matches!(
        next_status,
        TaskStatus::InProgress | TaskStatus::InReview | TaskStatus::Blocked | TaskStatus::Cancelled
    )
}

fn manager_can_change_status(next_status: TaskStatus) -> bool {
    matches!(
        next_status,
        TaskStatus::InProgress | TaskStatus::Completed | TaskStatus::Cancelled
    )
}

fn normalize_requested_status(
    actor: &User,
    task: &Task,
    requested_status: TaskStatus,
) -> AppResult<TaskStatus> {
    if requested_status == TaskStatus::Completed && task.review_required() {
        let is_creator = actor.id == Some(task.created_by_user_id);
        if !is_creator && !actor.role.is_manager_or_admin() {
            return Ok(TaskStatus::InReview);
        }
    }

    if requested_status == TaskStatus::Completed
        && task.review_required()
        && task.status != TaskStatus::InReview
    {
        return Err(AppError::business_rule(
            "TASK_REVIEW_REQUIRED",
            "Task must go through review before final completion",
            json!({ "task_uid": task.task_uid }),
        ));
    }

    Ok(requested_status)
}

fn build_status_message(previous_status: TaskStatus, next_status: TaskStatus) -> String {
    if next_status == TaskStatus::InReview {
        return format!(
            "Задача отправлена на проверку: {} -> {}",
            previous_status, next_status
        );
    }

    format!(
        "Статус задачи обновлён: {} -> {}",
        previous_status, next_status
    )
}

fn build_participant_notification(task: &Task, next_status: TaskStatus) -> String {
    match next_status {
        TaskStatus::InReview => {
            format!("Задача «{}» готова и ждёт вашей проверки.", task.title)
        }
        TaskStatus::Completed => format!("Задача «{}» принята и завершена.", task.title),
        TaskStatus::Blocked => format!("По задаче «{}» зафиксирован блокер.", task.title),
        TaskStatus::Cancelled => format!("Задача «{}» отменена.", task.title),
        TaskStatus::Created | TaskStatus::Sent | TaskStatus::InProgress => {
            format!("Задача «{}» теперь в статусе {}.", task.title, next_status)
        }
    }
}
