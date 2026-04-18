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
use crate::domain::notification::{Notification, NotificationType};
use crate::domain::task::{Task, TaskStatus};
use crate::shared::constants::reliability::{
    MAX_CONCURRENT_NOTIFICATION_DELIVERIES, MAX_NOTIFICATION_RETRY_ATTEMPTS,
    NOTIFICATION_RETRY_BASE_SECONDS, NOTIFICATION_RETRY_MAX_SECONDS,
    PENDING_NOTIFICATION_BATCH_SIZE,
};

const RECIPIENT_MISSING_ERROR: &str = "RECIPIENT_MISSING";
const CHAT_ID_MISSING_ERROR: &str = "CHAT_ID_MISSING";
const TELEGRAM_SEND_FAILED_ERROR: &str = "TELEGRAM_SEND_FAILED";
/// Permanent delivery failure — bot blocked, user deactivated, or chat gone.
/// These error codes are emitted by `TeloxideNotifier` (bot_gateway.rs).
const PERMANENT_DELIVERY_ERROR_CODES: &[&str] =
    &["TELEGRAM_BOT_BLOCKED", "TELEGRAM_CHAT_NOT_FOUND"];

#[derive(Clone)]
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

    /// Fetches the pending-notification batch and delivers each one concurrently
    /// (up to [`MAX_CONCURRENT_NOTIFICATION_DELIVERIES`] in flight at once).
    ///
    /// Repository failures from individual deliveries are collected; the first
    /// such failure is returned after the rest of the batch has finished, so a
    /// single stuck notification never blocks the others.
    pub async fn execute(&self) -> AppResult<()> {
        let notifications = self
            .notification_repository
            .list_pending(PENDING_NOTIFICATION_BATCH_SIZE)
            .await?;

        let semaphore = Arc::new(tokio::sync::Semaphore::new(
            MAX_CONCURRENT_NOTIFICATION_DELIVERIES,
        ));
        let mut join_set = tokio::task::JoinSet::new();

        for notification in notifications {
            let uc = self.clone();
            // acquire_owned keeps the permit alive until the spawned task drops it
            let permit = Arc::clone(&semaphore)
                .acquire_owned()
                .await
                .expect("semaphore is never closed");
            join_set.spawn(async move {
                let _permit = permit;
                uc.deliver_single(notification).await
            });
        }

        let mut first_error: Option<crate::domain::errors::AppError> = None;
        while let Some(result) = join_set.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::error!(code = error.code(), "notification_delivery_repo_error");
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
                Err(join_error) => {
                    tracing::error!(%join_error, "notification_delivery_task_panicked");
                }
            }
        }

        first_error.map_or(Ok(()), Err)
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    async fn deliver_single(&self, notification: Notification) -> AppResult<()> {
        let Some(notification_id) = notification.id else {
            return Ok(());
        };

        let Some(user) = self
            .user_repository
            .find_by_id(notification.recipient_user_id)
            .await?
        else {
            self.mark_failed(notification_id, RECIPIENT_MISSING_ERROR)
                .await?;
            return Ok(());
        };
        let Some(chat_id) = user.last_chat_id else {
            self.mark_failed(notification_id, CHAT_ID_MISSING_ERROR)
                .await?;
            return Ok(());
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
            Err(error) if PERMANENT_DELIVERY_ERROR_CODES.contains(&error.code()) => {
                // Bot blocked / chat gone — no point retrying; mark permanently failed.
                tracing::warn!(
                    code = error.code(),
                    notification_id,
                    recipient_user_id = notification.recipient_user_id,
                    "permanent telegram delivery failure"
                );
                self.mark_failed(notification_id, error.code()).await?;
            }
            Err(_) => {
                self.handle_transient_failure(notification_id, notification.attempt_count + 1)
                    .await?;
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

    /// Schedules a retry with exponential back-off, or permanently fails when
    /// the attempt limit is reached.
    ///
    /// Back-off formula: `base * 2^(attempt - 1)`, capped at
    /// [`NOTIFICATION_RETRY_MAX_SECONDS`].
    ///   attempt 1 → base (60 s)
    ///   attempt 2 → 2 × base (120 s)
    ///   attempt 3 → 4 × base (240 s)  ← last retry before permanent failure
    async fn handle_transient_failure(
        &self,
        notification_id: i64,
        next_attempt_count: i32,
    ) -> AppResult<()> {
        if next_attempt_count >= MAX_NOTIFICATION_RETRY_ATTEMPTS {
            self.mark_failed(notification_id, TELEGRAM_SEND_FAILED_ERROR)
                .await?;
            return Ok(());
        }

        let exponent = (next_attempt_count - 1).max(0) as u32;
        // 2^exponent: cap the exponent at 30 to prevent overflow (2^30 ≈ 10^9 fits in i64).
        let backoff_factor = 2i64.pow(exponent.min(30));
        let delay_seconds = NOTIFICATION_RETRY_BASE_SECONDS
            .saturating_mul(backoff_factor)
            .min(NOTIFICATION_RETRY_MAX_SECONDS);
        let next_attempt_at = Utc::now() + Duration::seconds(delay_seconds);
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
