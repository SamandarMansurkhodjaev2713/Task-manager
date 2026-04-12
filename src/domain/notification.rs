use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationType {
    TaskAssigned,
    TaskUpdated,
    DeadlineReminder,
    TaskCompleted,
    TaskCancelled,
    TaskReviewRequested,
    TaskBlocked,
    DailySummary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryState {
    Pending,
    Sent,
    RetryPending,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: Option<i64>,
    pub task_id: Option<i64>,
    pub recipient_user_id: i64,
    pub notification_type: NotificationType,
    pub message: String,
    pub dedupe_key: String,
    pub telegram_message_id: Option<i32>,
    pub delivery_state: NotificationDeliveryState,
    pub is_sent: bool,
    pub is_read: bool,
    pub attempt_count: i32,
    pub sent_at: Option<DateTime<Utc>>,
    pub read_at: Option<DateTime<Utc>>,
    pub next_attempt_at: Option<DateTime<Utc>>,
    pub last_error_code: Option<String>,
    pub created_at: DateTime<Utc>,
}
