use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::application::dto::task_views::{ClarificationRequest, TaskStatusSummary};
use crate::application::ports::repositories::{
    AuditLogRepository, NotificationRepository, TaskRepository,
};
use crate::application::ports::services::Clock;
use crate::application::use_cases::assignee_resolution::{AssigneeResolution, AssigneeResolver};
use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::user::User;

pub enum ReassignTaskOutcome {
    Reassigned(TaskStatusSummary),
    ClarificationRequired(ClarificationRequest),
}

pub struct ReassignTaskUseCase {
    clock: Arc<dyn Clock>,
    task_repository: Arc<dyn TaskRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
    assignee_resolver: Arc<AssigneeResolver>,
}

impl ReassignTaskUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        task_repository: Arc<dyn TaskRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
        audit_log_repository: Arc<dyn AuditLogRepository>,
        assignee_resolver: Arc<AssigneeResolver>,
    ) -> Self {
        Self {
            clock,
            task_repository,
            notification_repository,
            audit_log_repository,
            assignee_resolver,
        }
    }

    pub async fn execute(
        &self,
        actor: &User,
        task_uid: Uuid,
        assignee_query: &str,
    ) -> AppResult<ReassignTaskOutcome> {
        let Some(actor_id) = actor.id else {
            return Err(AppError::unauthenticated(
                "User must be registered before reassigning tasks",
                json!({ "telegram_id": actor.telegram_id }),
            ));
        };
        let Some(task) = self.task_repository.find_by_uid(task_uid).await? else {
            return Err(AppError::not_found(
                "TASK_NOT_FOUND",
                "Task was not found",
                json!({ "task_uid": task_uid }),
            ));
        };
        authorize_reassign(actor, &task)?;

        let resolution = self.assignee_resolver.resolve(assignee_query).await?;
        let AssigneeResolution::Resolved { user, employee } = resolution else {
            let AssigneeResolution::ClarificationRequired(request) = resolution else {
                unreachable!("resolver must produce a known outcome");
            };
            return Ok(ReassignTaskOutcome::ClarificationRequired(request));
        };

        let reassigned_task = task.reassign(
            user.as_ref().and_then(|value| value.id),
            employee.as_ref().and_then(|value| value.id),
            self.clock.now_utc(),
        )?;
        let saved_task = self.task_repository.update(&reassigned_task).await?;
        self.log_reassignment(actor_id, &task, &saved_task, assignee_query)
            .await?;
        self.notify_new_assignee(&saved_task).await?;

        Ok(ReassignTaskOutcome::Reassigned(TaskStatusSummary {
            task_uid,
            status: saved_task.status,
            message: format!("Исполнитель обновлён: {}", assignee_query.trim()),
        }))
    }

    async fn log_reassignment(
        &self,
        actor_id: i64,
        previous_task: &crate::domain::task::Task,
        saved_task: &crate::domain::task::Task,
        assignee_query: &str,
    ) -> AppResult<()> {
        let Some(task_id) = saved_task.id else {
            return Err(AppError::internal(
                "TASK_ID_MISSING",
                "Task must have an identifier after reassignment",
                json!({ "task_uid": saved_task.task_uid }),
            ));
        };
        let entry = AuditLogEntry {
            id: None,
            task_id,
            action: AuditAction::Reassigned,
            old_status: Some(previous_task.status.to_string()),
            new_status: Some(saved_task.status.to_string()),
            changed_by_user_id: Some(actor_id),
            metadata: json!({
                "query": assignee_query.trim(),
                "assigned_to_user_id": saved_task.assigned_to_user_id,
                "assigned_to_employee_id": saved_task.assigned_to_employee_id,
            }),
            created_at: self.clock.now_utc(),
        };
        let _ = self.audit_log_repository.append(&entry).await?;
        Ok(())
    }

    async fn notify_new_assignee(&self, task: &crate::domain::task::Task) -> AppResult<()> {
        let Some(recipient_user_id) = task.assigned_to_user_id else {
            return Ok(());
        };

        let notification = Notification {
            id: None,
            task_id: task.id,
            recipient_user_id,
            notification_type: NotificationType::TaskAssigned,
            message: format!("Вам переназначили задачу «{}».", task.title),
            dedupe_key: format!(
                "task_reassigned:{}:{}:{}",
                task.task_uid, recipient_user_id, task.version
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
        Ok(())
    }
}

fn authorize_reassign(actor: &User, task: &crate::domain::task::Task) -> AppResult<()> {
    let Some(actor_id) = actor.id else {
        return Err(AppError::unauthenticated(
            "User must be registered before reassigning tasks",
            json!({ "telegram_id": actor.telegram_id }),
        ));
    };

    if actor.role.is_manager_or_admin() || actor_id == task.created_by_user_id {
        return Ok(());
    }

    Err(AppError::unauthorized(
        "Only the creator, manager or admin can reassign a task",
        json!({ "task_uid": task.task_uid }),
    ))
}
