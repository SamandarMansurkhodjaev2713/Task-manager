use std::sync::Arc;

use crate::application::ports::repositories::{
    NotificationRepository, TaskRepository, UserRepository,
};
use crate::application::ports::services::Clock;
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::task::TaskStatus;
use crate::shared::constants::limits::{
    DAILY_SUMMARY_OPEN_TASK_SCAN_MULTIPLIER, MAX_DAILY_SUMMARY_PREVIEW_TASKS,
    MAX_DAILY_SUMMARY_TASKS,
};

pub struct EnqueueDailySummariesUseCase {
    clock: Arc<dyn Clock>,
    user_repository: Arc<dyn UserRepository>,
    task_repository: Arc<dyn TaskRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
}

impl EnqueueDailySummariesUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        user_repository: Arc<dyn UserRepository>,
        task_repository: Arc<dyn TaskRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
    ) -> Self {
        Self {
            clock,
            user_repository,
            task_repository,
            notification_repository,
        }
    }

    pub async fn execute(&self) -> crate::domain::errors::AppResult<()> {
        let today = self.clock.today_utc();
        let users = self.user_repository.list_with_chat_id().await?;
        let open_tasks = self
            .task_repository
            .list_open((MAX_DAILY_SUMMARY_TASKS * DAILY_SUMMARY_OPEN_TASK_SCAN_MULTIPLIER) as i64)
            .await?;

        for user in users {
            let Some(user_id) = user.id else {
                continue;
            };
            let assigned_tasks = open_tasks
                .iter()
                .filter(|task| task.assigned_to_user_id == Some(user_id))
                .take(MAX_DAILY_SUMMARY_TASKS)
                .collect::<Vec<_>>();
            if assigned_tasks.is_empty() {
                continue;
            }

            let overdue_count = assigned_tasks
                .iter()
                .filter(|task| task.deadline.is_some_and(|deadline| deadline < today))
                .count();
            let today_count = assigned_tasks
                .iter()
                .filter(|task| task.deadline == Some(today))
                .count();
            let blocked_count = assigned_tasks
                .iter()
                .filter(|task| task.status == TaskStatus::Blocked)
                .count();
            let review_count = assigned_tasks
                .iter()
                .filter(|task| task.status == TaskStatus::InReview)
                .count();

            let preview = assigned_tasks
                .iter()
                .map(|task| format!("• {}", task.title))
                .take(MAX_DAILY_SUMMARY_PREVIEW_TASKS)
                .collect::<Vec<_>>()
                .join("\n");

            let message = format!(
                "Доброе утро. В работе у вас {} задач.\n\nПросрочено: {}\nНа сегодня: {}\nС блокером: {}\nНа проверке: {}\n\nБлижайшие задачи:\n{}",
                assigned_tasks.len(),
                overdue_count,
                today_count,
                blocked_count,
                review_count,
                preview
            );

            let notification = Notification {
                id: None,
                task_id: None,
                recipient_user_id: user_id,
                notification_type: NotificationType::DailySummary,
                message,
                dedupe_key: format!("daily_summary:{}:{}", user_id, today),
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
