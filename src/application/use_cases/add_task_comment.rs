use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::application::policies::role_authorization::RoleAuthorizationPolicy;
use crate::application::ports::repositories::{
    AuditLogRepository, CommentRepository, NotificationRepository, TaskRepository,
};
use crate::application::ports::services::Clock;
use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::comment::{CommentKind, TaskComment};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::user::User;

pub struct AddTaskCommentUseCase {
    clock: Arc<dyn Clock>,
    task_repository: Arc<dyn TaskRepository>,
    comment_repository: Arc<dyn CommentRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
}

impl AddTaskCommentUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        task_repository: Arc<dyn TaskRepository>,
        comment_repository: Arc<dyn CommentRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
        audit_log_repository: Arc<dyn AuditLogRepository>,
    ) -> Self {
        Self {
            clock,
            task_repository,
            comment_repository,
            notification_repository,
            audit_log_repository,
        }
    }

    pub async fn execute(&self, actor: &User, task_uid: Uuid, body: &str) -> AppResult<String> {
        let Some(actor_id) = actor.id else {
            return Err(AppError::unauthenticated(
                "User must be registered before commenting on a task",
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
        RoleAuthorizationPolicy::ensure_can_comment(actor, &task)?;

        let Some(task_id) = task.id else {
            return Err(AppError::internal(
                "TASK_ID_MISSING",
                "Task must have an identifier before creating comments",
                json!({ "task_uid": task_uid }),
            ));
        };

        let comment = TaskComment::new(
            task_id,
            actor_id,
            CommentKind::Context,
            body,
            self.clock.now_utc(),
        )?;
        let stored_comment = self.comment_repository.create(&comment).await?;
        self.log_comment(task_id, actor_id, &stored_comment.body)
            .await?;
        self.notify_other_side(actor_id, &task, &stored_comment.body, stored_comment.id)
            .await?;

        Ok("Комментарий добавлен.".to_owned())
    }

    async fn log_comment(&self, task_id: i64, actor_id: i64, body: &str) -> AppResult<()> {
        let entry = AuditLogEntry {
            id: None,
            task_id,
            action: AuditAction::Commented,
            old_status: None,
            new_status: None,
            changed_by_user_id: Some(actor_id),
            metadata: json!({ "preview": body }),
            created_at: self.clock.now_utc(),
        };
        let _ = self.audit_log_repository.append(&entry).await?;
        Ok(())
    }

    async fn notify_other_side(
        &self,
        actor_id: i64,
        task: &crate::domain::task::Task,
        body: &str,
        comment_id: Option<i64>,
    ) -> AppResult<()> {
        let recipients = [Some(task.created_by_user_id), task.assigned_to_user_id];
        for recipient_user_id in recipients.into_iter().flatten() {
            if recipient_user_id == actor_id {
                continue;
            }

            let notification = Notification {
                id: None,
                task_id: task.id,
                recipient_user_id,
                notification_type: NotificationType::TaskUpdated,
                message: format!("Новый комментарий по задаче «{}»: {}", task.title, body),
                dedupe_key: format!(
                    "task_comment:{}:{}:{}",
                    task.task_uid,
                    recipient_user_id,
                    comment_id.unwrap_or_default()
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
