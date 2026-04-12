use teloxide::payloads::{AnswerCallbackQuerySetters, SendMessageSetters};
use teloxide::prelude::{CallbackQuery, Message, Requester};
use teloxide::types::{ChatId, InlineKeyboardMarkup};
use teloxide::Bot;

use crate::domain::errors::AppError;
use crate::domain::message::{IncomingMessage, MessageContent, VoiceAttachment};

use super::RATE_LIMIT_MESSAGE;

pub(crate) async fn send_screen(
    bot: &Bot,
    chat_id: ChatId,
    text: &str,
    keyboard: InlineKeyboardMarkup,
) -> Result<(), teloxide::RequestError> {
    bot.send_message(chat_id, text.to_owned())
        .reply_markup(keyboard)
        .await?;
    Ok(())
}

pub(crate) async fn answer_callback(
    bot: &Bot,
    callback_id: &str,
    text: &str,
) -> Result<(), teloxide::RequestError> {
    bot.answer_callback_query(callback_id.to_owned())
        .text(text.to_owned())
        .await?;
    Ok(())
}

pub(crate) async fn send_error(
    bot: &Bot,
    chat_id: i64,
    error: AppError,
) -> Result<(), teloxide::RequestError> {
    let message = match &error {
        AppError::NotFound { .. } => format!("Не найдено: {}", error.message()),
        AppError::Auth { code, .. } if *code == "UNAUTHORIZED" => {
            "Недостаточно прав для выполнения этого действия.".to_owned()
        }
        AppError::Auth { .. } => "Сначала выполните /start, чтобы зарегистрировать чат.".to_owned(),
        AppError::Validation { .. } => format!("Некорректный запрос: {}", error.message()),
        AppError::RateLimit { .. } => RATE_LIMIT_MESSAGE.to_owned(),
        AppError::Conflict { .. } => format!("Конфликт: {}", error.message()),
        _ => "Произошла ошибка. Попробуйте повторить позже.".to_owned(),
    };

    bot.send_message(ChatId(chat_id), message).await?;
    Ok(())
}

pub(crate) fn to_incoming_message(message: &Message) -> Option<IncomingMessage> {
    let sender = message.from.as_ref()?;
    let content = if let Some(text) = message.text() {
        if text.starts_with('/') {
            MessageContent::Command {
                text: text.to_owned(),
            }
        } else {
            MessageContent::Text {
                text: text.to_owned(),
            }
        }
    } else if let Some(voice) = message.voice() {
        MessageContent::Voice {
            voice: VoiceAttachment {
                file_id: voice.file.id.to_string(),
                file_unique_id: voice.file.unique_id.to_string(),
                duration_seconds: voice.duration.seconds() as u32,
                mime_type: voice.mime_type.clone().map(|value| value.to_string()),
                file_size_bytes: match voice.file.size {
                    u32::MAX => None,
                    size => Some(u64::from(size)),
                },
            },
        }
    } else {
        return None;
    };

    Some(IncomingMessage {
        message_id: message.id.0,
        chat_id: message.chat.id.0,
        sender_id: i64::try_from(sender.id.0).ok()?,
        sender_name: sender.full_name(),
        sender_username: sender.username.clone(),
        content,
        timestamp: chrono::Utc::now(),
        source_message_key_override: None,
    })
}

pub(crate) fn callback_to_incoming_message(
    callback_query: &CallbackQuery,
) -> Option<IncomingMessage> {
    let sender = &callback_query.from;
    let message = callback_query.message.as_ref()?;

    Some(IncomingMessage {
        message_id: message.id().0,
        chat_id: message.chat().id.0,
        sender_id: i64::try_from(sender.id.0).ok()?,
        sender_name: sender.full_name(),
        sender_username: sender.username.clone(),
        content: MessageContent::Command {
            text: callback_query.data.clone().unwrap_or_default(),
        },
        timestamp: chrono::Utc::now(),
        source_message_key_override: None,
    })
}
