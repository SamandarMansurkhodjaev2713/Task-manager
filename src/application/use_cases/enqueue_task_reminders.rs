use std::sync::Arc;

use chrono::Duration;

use crate::application::ports::repositories::{NotificationRepository, TaskRepository};
use crate::application::ports::services::Clock;
use crate::domain::errors::AppResult;
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::shared::constants::limits::REMINDER_TASK_FETCH_LIMIT;

pub struct EnqueueTaskRemindersUseCase {
    clock: Arc<dyn Clock>,
    task_repository: Arc<dyn TaskRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
}

impl EnqueueTaskRemindersUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        task_repository: Arc<dyn TaskRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
    ) -> Self {
        Self {
            clock,
            task_repository,
            notification_repository,
        }
    }

    pub async fn enqueue_upcoming_deadlines(&self) -> AppResult<()> {
        let today = self.clock.today_utc();
        let tomorrow = today + Duration::days(1);
        let tasks = self
            .task_repository
            .get_due_between(tomorrow, tomorrow, REMINDER_TASK_FETCH_LIMIT)
            .await?;

        for task in tasks {
            let Some(recipient_user_id) = task.assigned_to_user_id else {
                continue;
            };

            let notification = Notification {
                id: None,
                task_id: task.id,
                recipient_user_id,
                notification_type: NotificationType::DeadlineReminder,
                message: format!(
                    "Напоминание: задача «{}» должна быть готова завтра.",
                    task.title
                ),
                dedupe_key: format!("deadline_reminder:{}:{}", task.task_uid, tomorrow),
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

    pub async fn enqueue_overdue_alerts(&self) -> AppResult<()> {
        let today = self.clock.today_utc();
        let tasks = self
            .task_repository
            .get_overdue(today, REMINDER_TASK_FETCH_LIMIT)
            .await?;

        for task in tasks {
            let Some(deadline) = task.deadline else {
                continue;
            };
            let overdue_days = (today - deadline).num_days();
            if overdue_days % 3 != 0 {
                continue;
            }

            let recipients = [Some(task.created_by_user_id), task.assigned_to_user_id];
            for recipient_user_id in recipients.into_iter().flatten() {
                let notification = Notification {
                    id: None,
                    task_id: task.id,
                    recipient_user_id,
                    notification_type: NotificationType::TaskUpdated,
                    message: format!(
                        "Задача «{}» просрочена уже на {} дн.",
                        task.title, overdue_days
                    ),
                    dedupe_key: format!(
                        "overdue:{}:{}:{}",
                        task.task_uid, recipient_user_id, overdue_days
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
        }

        Ok(())
    }
}
