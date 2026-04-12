use std::sync::Arc;

use chrono::{Duration, Utc};
use metrics::counter;
use serde_json::json;

use crate::application::ports::repositories::{
    AuditLogRepository, NotificationRepository, TaskRepository, UserRepository,
};
use crate::application::ports::services::TelegramNotifier;
use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::errors::AppResult;
use crate::domain::notification::NotificationType;
use crate::domain::task::{Task, TaskStatus};
use crate::shared::constants::reliability::{
    MAX_NOTIFICATION_RETRY_ATTEMPTS, NOTIFICATION_RETRY_DELAY_SECONDS,
    PENDING_NOTIFICATION_BATCH_SIZE,
};

const RECIPIENT_MISSING_ERROR: &str = "RECIPIENT_MISSING";
const CHAT_ID_MISSING_ERROR: &str = "CHAT_ID_MISSING";
const TELEGRAM_SEND_FAILED_ERROR: &str = "TELEGRAM_SEND_FAILED";

pub struct ProcessNotificationsUseCase {
    notification_repository: Arc<dyn NotificationRepository>,
    user_repository: Arc<dyn UserRepository>,
    task_repository: Arc<dyn TaskRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
    telegram_notifier: Arc<dyn TelegramNotifier>,
}

impl ProcessNotificationsUseCase {
    pub fn new(
        notification_repository: Arc<dyn NotificationRepository>,
        user_repository: Arc<dyn UserRepository>,
        task_repository: Arc<dyn TaskRepository>,
        audit_log_repository: Arc<dyn AuditLogRepository>,
        telegram_notifier: Arc<dyn TelegramNotifier>,
    ) -> Self {
        Self {
            notification_repository,
            user_repository,
            task_repository,
            audit_log_repository,
            telegram_notifier,
        }
    }

    pub async fn execute(&self) -> AppResult<()> {
        let notifications = self
            .notification_repository
            .list_pending(PENDING_NOTIFICATION_BATCH_SIZE)
            .await?;

        for notification in notifications {
            let Some(notification_id) = notification.id else {
                continue;
            };

            let Some(user) = self
                .user_repository
                .find_by_id(notification.recipient_user_id)
                .await?
            else {
                self.mark_failed(notification_id, RECIPIENT_MISSING_ERROR)
                    .await?;
                continue;
            };
            let Some(chat_id) = user.last_chat_id else {
                self.mark_failed(notification_id, CHAT_ID_MISSING_ERROR)
                    .await?;
                continue;
            };
            let related_task = self.load_related_task(notification.task_id).await?;

            match self
                .telegram_notifier
                .send_notification(
                    chat_id,
                    &notification.message,
                    notification.notification_type,
                    related_task.as_ref().map(|task| task.task_uid),
                    related_task.as_ref().map(|task| task.status),
                )
                .await
            {
                Ok(message_id) => {
                    self.notification_repository
                        .mark_sent(notification_id, message_id.0, Utc::now())
                        .await?;
                    self.mark_related_task_as_sent(
                        notification.notification_type,
                        related_task.as_ref(),
                    )
                    .await?;
                    counter!("notification_delivery_total", "status" => "sent").increment(1);
                }
                Err(_) => {
                    self.handle_delivery_failure(notification_id, notification.attempt_count + 1)
                        .await?;
                }
            }
        }

        Ok(())
    }

    async fn load_related_task(&self, task_id: Option<i64>) -> AppResult<Option<Task>> {
        let Some(task_id) = task_id else {
            return Ok(None);
        };
        self.task_repository.find_by_id(task_id).await
    }

    async fn handle_delivery_failure(
        &self,
        notification_id: i64,
        next_attempt_count: i32,
    ) -> AppResult<()> {
        if next_attempt_count >= MAX_NOTIFICATION_RETRY_ATTEMPTS {
            self.mark_failed(notification_id, TELEGRAM_SEND_FAILED_ERROR)
                .await?;
            return Ok(());
        }

        let next_attempt_at = Utc::now() + Duration::seconds(NOTIFICATION_RETRY_DELAY_SECONDS);
        self.notification_repository
            .mark_retry_pending(notification_id, next_attempt_at, TELEGRAM_SEND_FAILED_ERROR)
            .await?;
        counter!("notification_delivery_total", "status" => "retry_pending").increment(1);
        Ok(())
    }

    async fn mark_failed(&self, notification_id: i64, error_code: &'static str) -> AppResult<()> {
        self.notification_repository
            .mark_failed(notification_id, error_code)
            .await?;
        counter!("notification_delivery_total", "status" => "failed").increment(1);
        Ok(())
    }

    async fn mark_related_task_as_sent(
        &self,
        notification_type: NotificationType,
        task: Option<&Task>,
    ) -> AppResult<()> {
        if !matches!(notification_type, NotificationType::TaskAssigned) {
            return Ok(());
        }

        let Some(task) = task else {
            return Ok(());
        };
        if task.status != TaskStatus::Created {
            return Ok(());
        }

        let now = Utc::now();
        let updated_task = task.transition_to(TaskStatus::Sent, now)?;
        let stored_task = match self.task_repository.update(&updated_task).await {
            Ok(task) => task,
            Err(error) if error.code() == "TASK_VERSION_CONFLICT" => return Ok(()),
            Err(error) => return Err(error),
        };
        let Some(task_id) = stored_task.id else {
            return Ok(());
        };

        let entry = AuditLogEntry {
            id: None,
            task_id,
            action: AuditAction::Sent,
            old_status: Some(TaskStatus::Created.to_string()),
            new_status: Some(TaskStatus::Sent.to_string()),
            changed_by_user_id: None,
            metadata: json!({ "delivery_channel": "telegram" }),
            created_at: now,
        };
        let _ = self.audit_log_repository.append(&entry).await?;
        Ok(())
    }
}
