use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::application::ports::repositories::NotificationRepository;
use crate::domain::errors::AppResult;
use crate::domain::notification::{Notification, NotificationType};
use crate::infrastructure::db::models::NotificationRow;

use super::common::{
    bool_as_i64, database_error, delivery_state_to_db, notification_type_to_db,
    NOTIFICATION_COLUMNS,
};

#[derive(Clone)]
pub struct SqliteNotificationRepository {
    pool: SqlitePool,
}

impl SqliteNotificationRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl NotificationRepository for SqliteNotificationRepository {
    async fn enqueue(&self, notification: &Notification) -> AppResult<Notification> {
        let query = "INSERT OR IGNORE INTO notifications (
                task_id, recipient_user_id, notification_type, message, dedupe_key, telegram_message_id,
                delivery_state, is_sent, is_read, attempt_count, sent_at, read_at, next_attempt_at, last_error_code, created_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
            .to_owned();
        sqlx::query(&query)
            .bind(notification.task_id)
            .bind(notification.recipient_user_id)
            .bind(notification_type_to_db(notification.notification_type))
            .bind(&notification.message)
            .bind(&notification.dedupe_key)
            .bind(notification.telegram_message_id)
            .bind(delivery_state_to_db(notification.delivery_state))
            .bind(bool_as_i64(notification.is_sent))
            .bind(bool_as_i64(notification.is_read))
            .bind(notification.attempt_count)
            .bind(notification.sent_at)
            .bind(notification.read_at)
            .bind(notification.next_attempt_at)
            .bind(&notification.last_error_code)
            .bind(notification.created_at)
            .execute(&self.pool)
            .await
            .map_err(database_error)?;

        let lookup =
            format!("SELECT {NOTIFICATION_COLUMNS} FROM notifications WHERE dedupe_key = ?");
        let row = sqlx::query_as::<_, NotificationRow>(&lookup)
            .bind(&notification.dedupe_key)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_pending(&self, limit: i64) -> AppResult<Vec<Notification>> {
        let query = format!(
            "SELECT {NOTIFICATION_COLUMNS} FROM notifications
             WHERE delivery_state IN ('pending', 'retry_pending')
               AND (next_attempt_at IS NULL OR next_attempt_at <= ?)
             ORDER BY created_at ASC, id ASC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, NotificationRow>(&query)
            .bind(Utc::now())
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn mark_sent(
        &self,
        notification_id: i64,
        telegram_message_id: i32,
        sent_at_utc: DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE notifications
             SET is_sent = 1, delivery_state = 'sent', telegram_message_id = ?, sent_at = ?, next_attempt_at = NULL, last_error_code = NULL
             WHERE id = ?",
        )
        .bind(telegram_message_id)
        .bind(sent_at_utc)
        .bind(notification_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn mark_retry_pending(
        &self,
        notification_id: i64,
        next_attempt_at: DateTime<Utc>,
        error_code: &'static str,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE notifications
             SET attempt_count = attempt_count + 1,
                 delivery_state = 'retry_pending',
                 next_attempt_at = ?,
                 last_error_code = ?
             WHERE id = ?",
        )
        .bind(next_attempt_at)
        .bind(error_code)
        .bind(notification_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn mark_failed(&self, notification_id: i64, error_code: &'static str) -> AppResult<()> {
        sqlx::query(
            "UPDATE notifications
             SET attempt_count = attempt_count + 1,
                 delivery_state = 'failed',
                 next_attempt_at = NULL,
                 last_error_code = ?
             WHERE id = ?",
        )
        .bind(error_code)
        .bind(notification_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn requeue(&self, notification_id: i64) -> AppResult<()> {
        sqlx::query(
            "UPDATE notifications
             SET delivery_state = 'pending',
                 next_attempt_at = NULL,
                 last_error_code = NULL
             WHERE id = ?",
        )
        .bind(notification_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }

    async fn find_latest_for_task_and_recipient(
        &self,
        task_id: i64,
        recipient_user_id: i64,
        notification_type: NotificationType,
    ) -> AppResult<Option<Notification>> {
        let query = format!(
            "SELECT {NOTIFICATION_COLUMNS} FROM notifications
             WHERE task_id = ? AND recipient_user_id = ? AND notification_type = ?
             ORDER BY created_at DESC, id DESC
             LIMIT 1"
        );
        let row = sqlx::query_as::<_, NotificationRow>(&query)
            .bind(task_id)
            .bind(recipient_user_id)
            .bind(notification_type_to_db(notification_type))
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }
}
