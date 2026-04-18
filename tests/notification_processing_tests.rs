use std::sync::Arc;

use chrono::Utc;

use telegram_task_bot::application::use_cases::process_notifications::ProcessNotificationsUseCase;
use telegram_task_bot::domain::errors::AppError;
use telegram_task_bot::domain::notification::{
    Notification, NotificationDeliveryState, NotificationType,
};
use telegram_task_bot::domain::user::{User, UserRole};

mod mock_impls {
    use async_trait::async_trait;
    use teloxide::types::MessageId;

    use telegram_task_bot::application::ports::repositories::*;
    use telegram_task_bot::application::ports::services::TelegramNotifier;
    use telegram_task_bot::domain::audit::AuditLogEntry;
    use telegram_task_bot::domain::errors::AppResult;
    use telegram_task_bot::domain::notification::{Notification, NotificationType};
    use telegram_task_bot::domain::task::{Task, TaskStats};
    use telegram_task_bot::domain::user::User;
    use uuid::Uuid;

    // ── Minimal stubs that panic if unexpectedly called ────────────────────────

    mockall::mock! {
        pub NotifRepo {}
        #[async_trait]
        impl NotificationRepository for NotifRepo {
            async fn enqueue(&self, notification: &Notification) -> AppResult<Notification>;
            async fn list_pending(&self, limit: i64) -> AppResult<Vec<Notification>>;
            async fn mark_sent(
                &self,
                notification_id: i64,
                telegram_message_id: i32,
                sent_at_utc: chrono::DateTime<chrono::Utc>,
            ) -> AppResult<()>;
            async fn mark_retry_pending(
                &self,
                notification_id: i64,
                next_attempt_at: chrono::DateTime<chrono::Utc>,
                error_code: &'static str,
            ) -> AppResult<()>;
            async fn mark_failed(
                &self,
                notification_id: i64,
                error_code: &'static str,
            ) -> AppResult<()>;
            async fn requeue(&self, notification_id: i64) -> AppResult<()>;
            async fn find_latest_for_task_and_recipient(
                &self,
                task_id: i64,
                recipient_user_id: i64,
                notification_type: NotificationType,
            ) -> AppResult<Option<Notification>>;
        }
    }

    mockall::mock! {
        pub UserRepo {}
        #[async_trait]
        impl UserRepository for UserRepo {
            async fn upsert_from_message(&self, user: &User) -> AppResult<User>;
            async fn find_by_id(&self, user_id: i64) -> AppResult<Option<User>>;
            async fn find_by_telegram_id(&self, telegram_id: i64) -> AppResult<Option<User>>;
            async fn find_by_username(&self, username: &str) -> AppResult<Option<User>>;
            async fn list_with_chat_id(&self) -> AppResult<Vec<User>>;
        }
    }

    mockall::mock! {
        pub TaskRepo {}
        #[async_trait]
        impl TaskRepository for TaskRepo {
            async fn create_if_absent(&self, task: &Task) -> AppResult<PersistedTask>;
            async fn find_by_id(&self, task_id: i64) -> AppResult<Option<Task>>;
            async fn find_by_uid(&self, task_uid: Uuid) -> AppResult<Option<Task>>;
            async fn update(&self, task: &Task) -> AppResult<Task>;
            async fn list_assigned_to_user(
                &self,
                user_id: i64,
                cursor: Option<String>,
                limit: u32,
            ) -> AppResult<Vec<Task>>;
            async fn list_open_assigned_to_employee_without_user(
                &self,
                employee_id: i64,
                limit: i64,
            ) -> AppResult<Vec<Task>>;
            async fn list_created_by_user(
                &self,
                user_id: i64,
                cursor: Option<String>,
                limit: u32,
            ) -> AppResult<Vec<Task>>;
            async fn list_all(&self, cursor: Option<String>, limit: u32) -> AppResult<Vec<Task>>;
            async fn get_due_between(
                &self,
                start: chrono::NaiveDate,
                end: chrono::NaiveDate,
                limit: i64,
            ) -> AppResult<Vec<Task>>;
            async fn get_overdue(&self, as_of: chrono::NaiveDate, limit: i64) -> AppResult<Vec<Task>>;
            async fn count_stats_for_user(&self, user_id: i64) -> AppResult<TaskStats>;
            async fn count_stats_global(&self) -> AppResult<TaskStats>;
            async fn list_open(&self, limit: i64) -> AppResult<Vec<Task>>;
        }
    }

    mockall::mock! {
        pub AuditRepo {}
        #[async_trait]
        impl AuditLogRepository for AuditRepo {
            async fn append(&self, entry: &AuditLogEntry) -> AppResult<AuditLogEntry>;
            async fn list_for_task(&self, task_id: i64) -> AppResult<Vec<AuditLogEntry>>;
        }
    }

    mockall::mock! {
        pub Notifier {}
        #[async_trait]
        impl TelegramNotifier for Notifier {
            async fn send_text(&self, chat_id: i64, text: &str) -> AppResult<MessageId>;
            async fn send_notification(
                &self,
                chat_id: i64,
                text: &str,
                notification_type: NotificationType,
                task_uid: Option<uuid::Uuid>,
                status: Option<telegram_task_bot::domain::task::TaskStatus>,
            ) -> AppResult<MessageId>;
        }
    }
}

use mock_impls::*;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn pending_notification(id: i64, attempt_count: i32) -> Notification {
    Notification {
        id: Some(id),
        task_id: None,
        recipient_user_id: 1,
        notification_type: NotificationType::TaskAssigned,
        message: "Вам назначена задача".to_owned(),
        dedupe_key: format!("key:{id}"),
        telegram_message_id: None,
        delivery_state: NotificationDeliveryState::Pending,
        is_sent: false,
        is_read: false,
        attempt_count,
        sent_at: None,
        read_at: None,
        next_attempt_at: None,
        last_error_code: None,
        created_at: Utc::now(),
    }
}

fn user_with_chat_id(chat_id: i64) -> User {
    User {
        id: Some(1),
        telegram_id: chat_id,
        last_chat_id: Some(chat_id),
        telegram_username: None,
        full_name: Some("Test User".to_owned()),
        linked_employee_id: None,
        is_employee: false,
        role: UserRole::User,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn permanent_error(code: &'static str) -> AppError {
    AppError::network(code, "Permanent failure", serde_json::json!({}))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// When the notifier returns a TELEGRAM_BOT_BLOCKED error, the notification
/// must be permanently failed without scheduling a retry (H-10).
#[tokio::test]
async fn given_bot_blocked_error_when_delivering_then_permanently_fails_without_retry() {
    let mut notif_repo = MockNotifRepo::new();
    notif_repo
        .expect_list_pending()
        .returning(|_| Ok(vec![pending_notification(1, 0)]));
    notif_repo
        .expect_mark_failed()
        .withf(|&id, code| id == 1 && code == "TELEGRAM_BOT_BLOCKED")
        .times(1)
        .returning(|_, _| Ok(()));
    // mark_retry_pending must NOT be called
    notif_repo.expect_mark_retry_pending().never();

    let mut user_repo = MockUserRepo::new();
    user_repo
        .expect_find_by_id()
        .returning(|_| Ok(Some(user_with_chat_id(42))));

    let mut task_repo = MockTaskRepo::new();
    task_repo.expect_find_by_id().returning(|_| Ok(None));

    let mut notifier = MockNotifier::new();
    notifier
        .expect_send_notification()
        .returning(|_, _, _, _, _| Err(permanent_error("TELEGRAM_BOT_BLOCKED")));

    let audit_repo = MockAuditRepo::new();

    let uc = ProcessNotificationsUseCase::new(
        Arc::new(notif_repo),
        Arc::new(user_repo),
        Arc::new(task_repo),
        Arc::new(audit_repo),
        Arc::new(notifier),
    );

    uc.execute().await.expect("execute should not error");
}

/// When the notifier returns a transient failure, the notification should be
/// scheduled for retry with exponential backoff — not permanently failed (M-23).
#[tokio::test]
async fn given_transient_error_on_first_attempt_when_delivering_then_schedules_retry() {
    let mut notif_repo = MockNotifRepo::new();
    notif_repo
        .expect_list_pending()
        .returning(|_| Ok(vec![pending_notification(2, 0)]));
    // Retry must be scheduled; permanent failure must NOT be called.
    notif_repo.expect_mark_failed().never();
    notif_repo
        .expect_mark_retry_pending()
        .withf(|&id, next_attempt_at, code| {
            let now = Utc::now();
            let delta = (*next_attempt_at - now).num_seconds();
            id == 2
                && code == "TELEGRAM_SEND_FAILED"
                // base delay = 60 s; allow ±5 s for test execution time
                && (50..=120).contains(&delta)
        })
        .times(1)
        .returning(|_, _, _| Ok(()));

    let mut user_repo = MockUserRepo::new();
    user_repo
        .expect_find_by_id()
        .returning(|_| Ok(Some(user_with_chat_id(42))));

    let mut task_repo = MockTaskRepo::new();
    task_repo.expect_find_by_id().returning(|_| Ok(None));

    let mut notifier = MockNotifier::new();
    notifier
        .expect_send_notification()
        .returning(|_, _, _, _, _| {
            Err(AppError::network(
                "TELEGRAM_REQUEST_FAILED",
                "transient error",
                serde_json::json!({}),
            ))
        });

    let audit_repo = MockAuditRepo::new();

    let uc = ProcessNotificationsUseCase::new(
        Arc::new(notif_repo),
        Arc::new(user_repo),
        Arc::new(task_repo),
        Arc::new(audit_repo),
        Arc::new(notifier),
    );

    uc.execute().await.expect("execute should not error");
}

/// When a notification has exhausted all retry attempts, a transient failure
/// must result in a permanent mark_failed (not another retry).
#[tokio::test]
async fn given_transient_error_on_last_attempt_when_delivering_then_permanently_fails() {
    let max_attempts = 3_i32; // MAX_NOTIFICATION_RETRY_ATTEMPTS
    let mut notif_repo = MockNotifRepo::new();
    notif_repo
        .expect_list_pending()
        .returning(move |_| Ok(vec![pending_notification(3, max_attempts - 1)]));
    notif_repo
        .expect_mark_failed()
        .withf(|&id, code| id == 3 && code == "TELEGRAM_SEND_FAILED")
        .times(1)
        .returning(|_, _| Ok(()));
    notif_repo.expect_mark_retry_pending().never();

    let mut user_repo = MockUserRepo::new();
    user_repo
        .expect_find_by_id()
        .returning(|_| Ok(Some(user_with_chat_id(42))));

    let mut task_repo = MockTaskRepo::new();
    task_repo.expect_find_by_id().returning(|_| Ok(None));

    let mut notifier = MockNotifier::new();
    notifier
        .expect_send_notification()
        .returning(|_, _, _, _, _| {
            Err(AppError::network(
                "TELEGRAM_REQUEST_FAILED",
                "transient error",
                serde_json::json!({}),
            ))
        });

    let audit_repo = MockAuditRepo::new();

    let uc = ProcessNotificationsUseCase::new(
        Arc::new(notif_repo),
        Arc::new(user_repo),
        Arc::new(task_repo),
        Arc::new(audit_repo),
        Arc::new(notifier),
    );

    uc.execute().await.expect("execute should not error");
}
