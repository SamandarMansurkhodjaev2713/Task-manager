use async_trait::async_trait;
use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::Requester;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId};
use teloxide::Bot;
use uuid::Uuid;

use crate::application::ports::services::TelegramNotifier;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::notification::NotificationType;
use crate::domain::task::TaskStatus;

const OPEN_TASK_LABEL: &str = "📋 Открыть";
const START_PROGRESS_LABEL: &str = "▶️ В работу";
const APPROVE_LABEL: &str = "✅ Принять";
const RETURN_TO_WORK_LABEL: &str = "↩️ Вернуть";
const BLOCKER_LABEL: &str = "🚧 Есть блокер";

#[derive(Clone)]
pub struct TeloxideNotifier {
    bot: Bot,
}

impl TeloxideNotifier {
    pub fn new(bot: Bot) -> Self {
        Self { bot }
    }

    pub fn bot(&self) -> Bot {
        self.bot.clone()
    }
}

#[async_trait]
impl TelegramNotifier for TeloxideNotifier {
    async fn send_text(&self, chat_id: i64, text: &str) -> AppResult<MessageId> {
        self.bot
            .send_message(teloxide::types::ChatId(chat_id), text.to_owned())
            .await
            .map(|message| message.id)
            .map_err(telegram_error)
    }

    async fn send_notification(
        &self,
        chat_id: i64,
        text: &str,
        notification_type: NotificationType,
        task_uid: Option<Uuid>,
        _status: Option<TaskStatus>,
    ) -> AppResult<MessageId> {
        let mut request = self
            .bot
            .send_message(teloxide::types::ChatId(chat_id), text.to_owned());

        if let Some(keyboard) = build_keyboard(notification_type, task_uid) {
            request = request.reply_markup(keyboard);
        }

        request
            .await
            .map(|message| message.id)
            .map_err(telegram_error)
    }
}

fn build_keyboard(
    notification_type: NotificationType,
    task_uid: Option<Uuid>,
) -> Option<InlineKeyboardMarkup> {
    let task_uid = task_uid?;
    let rows = match notification_type {
        NotificationType::TaskAssigned => vec![vec![
            callback_button(OPEN_TASK_LABEL, open_callback(task_uid)),
            callback_button(
                START_PROGRESS_LABEL,
                status_callback(task_uid, TaskStatus::InProgress),
            ),
            callback_button(BLOCKER_LABEL, block_callback(task_uid)),
        ]],
        NotificationType::TaskReviewRequested => vec![vec![
            callback_button(OPEN_TASK_LABEL, open_callback(task_uid)),
            callback_button(
                APPROVE_LABEL,
                status_callback(task_uid, TaskStatus::Completed),
            ),
            callback_button(
                RETURN_TO_WORK_LABEL,
                status_callback(task_uid, TaskStatus::InProgress),
            ),
        ]],
        NotificationType::TaskBlocked => vec![vec![callback_button(
            OPEN_TASK_LABEL,
            open_callback(task_uid),
        )]],
        NotificationType::DeadlineReminder | NotificationType::TaskUpdated => {
            vec![vec![callback_button(
                OPEN_TASK_LABEL,
                open_callback(task_uid),
            )]]
        }
        NotificationType::TaskCompleted | NotificationType::TaskCancelled => {
            vec![vec![callback_button(
                OPEN_TASK_LABEL,
                open_callback(task_uid),
            )]]
        }
        NotificationType::DailySummary => return None,
    };

    Some(InlineKeyboardMarkup::new(rows))
}

fn open_callback(task_uid: Uuid) -> String {
    format!("open:{task_uid}")
}

fn block_callback(task_uid: Uuid) -> String {
    format!("block:{task_uid}")
}

fn status_callback(task_uid: Uuid, status: TaskStatus) -> String {
    let status_code = match status {
        TaskStatus::Created => "created",
        TaskStatus::Sent => "sent",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::InReview => "in_review",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    };
    format!("status:{task_uid}:{status_code}")
}

fn callback_button(text: &str, payload: String) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(text.to_owned(), payload)
}

fn telegram_error(error: teloxide::RequestError) -> AppError {
    AppError::network(
        "TELEGRAM_REQUEST_FAILED",
        "Telegram API request failed",
        serde_json::json!({ "error": error.to_string() }),
    )
}
