use std::sync::Arc;

use serde_json::json;
use uuid::Uuid;

use crate::application::dto::task_views::TaskStatusSummary;
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
use crate::shared::task_codes::format_public_task_code_or_placeholder;

pub struct ReportTaskBlockerUseCase {
    clock: Arc<dyn Clock>,
    task_repository: Arc<dyn TaskRepository>,
    comment_repository: Arc<dyn CommentRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
}

impl ReportTaskBlockerUseCase {
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

    pub async fn execute(
        &self,
        actor: &User,
        task_uid: Uuid,
        blocker_reason: &str,
    ) -> AppResult<TaskStatusSummary> {
        let Some(actor_id) = actor.id else {
            return Err(AppError::unauthenticated(
                "User must be registered before reporting a blocker",
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
        RoleAuthorizationPolicy::ensure_can_report_blocker(actor, &task)?;

        let previous_status = task.status;
        let updated_task = task.apply_blocker(blocker_reason, self.clock.now_utc())?;
        let saved_task = self.task_repository.update(&updated_task).await?;
        let task_id = saved_task.id.ok_or_else(|| {
            AppError::internal(
                "TASK_ID_MISSING",
                "Task must have an identifier after blocker update",
                json!({ "task_uid": task_uid }),
            )
        })?;

        let blocker_comment = TaskComment::new(
            task_id,
            actor_id,
            CommentKind::Blocker,
            blocker_reason,
            self.clock.now_utc(),
        )?;
        let stored_comment = self.comment_repository.create(&blocker_comment).await?;
        self.log_blocker(
            task_id,
            actor_id,
            previous_status.to_string(),
            &stored_comment.body,
        )
        .await?;
        self.notify_creator(&saved_task, &stored_comment.body, actor_id)
            .await?;

        Ok(TaskStatusSummary {
            task_uid,
            public_code: format_public_task_code_or_placeholder(saved_task.id),
            status: saved_task.status,
            message: "Блокер сохранён, автор уведомлён.".to_owned(),
        })
    }

    async fn log_blocker(
        &self,
        task_id: i64,
        actor_id: i64,
        previous_status: String,
        blocker_reason: &str,
    ) -> AppResult<()> {
        let entry = AuditLogEntry {
            id: None,
            task_id,
            action: AuditAction::Blocked,
            old_status: Some(previous_status),
            new_status: Some("blocked".to_owned()),
            changed_by_user_id: Some(actor_id),
            metadata: json!({ "reason": blocker_reason }),
            created_at: self.clock.now_utc(),
        };
        let _ = self.audit_log_repository.append(&entry).await?;
        Ok(())
    }

    async fn notify_creator(
        &self,
        task: &crate::domain::task::Task,
        blocker_reason: &str,
        actor_id: i64,
    ) -> AppResult<()> {
        if task.created_by_user_id == actor_id {
            return Ok(());
        }

        let notification = Notification {
            id: None,
            task_id: task.id,
            recipient_user_id: task.created_by_user_id,
            notification_type: NotificationType::TaskBlocked,
            message: format!("По задаче «{}» есть блокер: {}", task.title, blocker_reason),
            dedupe_key: format!(
                "task_blocked:{}:{}:{}",
                task.task_uid, task.created_by_user_id, task.version
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
