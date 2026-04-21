use teloxide::payloads::{AnswerCallbackQuerySetters, EditMessageTextSetters, SendMessageSetters};
use teloxide::prelude::{CallbackQuery, Message, Requester};
use teloxide::types::{ChatId, InlineKeyboardMarkup, MessageId};
use teloxide::Bot;

use crate::domain::errors::AppError;
use crate::domain::message::{IncomingMessage, MessageContent, VoiceAttachment};
use crate::presentation::telegram::active_screens::{ActiveScreenState, ScreenDescriptor};

use super::{TelegramRuntime, RATE_LIMIT_MESSAGE};

/// Consult the per-update UX barrier (installed by the gateway in
/// `handle_message` / `handle_callback_query`) before rendering anything to
/// the user.  Returns `true` if this call is allowed to render.
///
/// Outside of an active update (background jobs, integration tests that bypass
/// the gateway), `current_barrier_for` returns `None` and we default to
/// "allowed" — the barrier is strictly about "one update, one UX effect", not
/// a global mutex.
async fn try_consume_update_barrier(
    state: &TelegramRuntime,
    chat_id: i64,
    site: &'static str,
) -> bool {
    let Some(barrier) = state.current_barrier_for(chat_id).await else {
        return true;
    };
    if barrier.try_consume() {
        return true;
    }
    tracing::warn!(
        chat_id,
        site,
        "UX barrier already consumed — suppressing duplicate outbound effect on the same update"
    );
    false
}

pub(crate) async fn send_screen(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    descriptor: ScreenDescriptor,
    text: &str,
    keyboard: InlineKeyboardMarkup,
) -> Result<(), teloxide::RequestError> {
    if !try_consume_update_barrier(state, chat_id.0, "send_screen").await {
        return Ok(());
    }
    if let Some(active_screen) = state.active_screens.get(chat_id.0).await {
        match try_edit_screen(
            bot,
            chat_id,
            active_screen.message_id,
            text,
            keyboard.clone(),
        )
        .await?
        {
            ScreenRenderResult::Edited | ScreenRenderResult::NoChange => {
                state
                    .active_screens
                    .set(
                        chat_id.0,
                        ActiveScreenState {
                            message_id: active_screen.message_id,
                            descriptor,
                        },
                    )
                    .await;
                return Ok(());
            }
            ScreenRenderResult::FallbackRequired => {}
        }
    }

    let sent_message = bot
        .send_message(chat_id, text.to_owned())
        .reply_markup(keyboard)
        .await?;
    state
        .active_screens
        .set(
            chat_id.0,
            ActiveScreenState {
                message_id: sent_message.id.0,
                descriptor,
            },
        )
        .await;
    Ok(())
}

pub(crate) async fn send_fresh_screen(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    descriptor: ScreenDescriptor,
    text: &str,
    keyboard: InlineKeyboardMarkup,
) -> Result<(), teloxide::RequestError> {
    if !try_consume_update_barrier(state, chat_id.0, "send_fresh_screen").await {
        return Ok(());
    }
    let sent_message = bot
        .send_message(chat_id, text.to_owned())
        .reply_markup(keyboard)
        .await?;
    state
        .active_screens
        .set(
            chat_id.0,
            ActiveScreenState {
                message_id: sent_message.id.0,
                descriptor,
            },
        )
        .await;
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
    state: &TelegramRuntime,
    chat_id: i64,
    error: AppError,
) -> Result<(), teloxide::RequestError> {
    // Log every application error so ops can trace root causes without exposing
    // internal details to the end user.
    tracing::warn!(
        code = error.code(),
        message = error.message(),
        "application error presented to user"
    );

    // Gate behind the per-update UX barrier: if another effect already went
    // out on this update (e.g. onboarding welcome), we MUST NOT follow it
    // with an error banner — that's exactly the "Welcome + Некорректный
    // запрос" regression we're fixing in P0.
    if !try_consume_update_barrier(state, chat_id, "send_error").await {
        return Ok(());
    }

    let message = match &error {
        AppError::NotFound { .. } => {
            "Не удалось найти запрошенный объект. Возможно, он был удалён или изменился.".to_owned()
        }
        AppError::Auth { code, .. } if *code == "UNAUTHORIZED" => {
            "Недостаточно прав для выполнения этого действия.".to_owned()
        }
        AppError::Auth { .. } => "Сначала выполните /start, чтобы зарегистрировать чат.".to_owned(),
        AppError::Validation { .. } => {
            "Некорректный запрос. Проверьте данные и попробуйте снова.".to_owned()
        }
        AppError::RateLimit { .. } => RATE_LIMIT_MESSAGE.to_owned(),
        AppError::Conflict { .. } => {
            "Данные уже изменились. Попробуйте обновить страницу и повторить действие.".to_owned()
        }
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
                duration_seconds: voice.duration.seconds(),
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
        is_voice_origin: false,
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
        is_voice_origin: false,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScreenRenderResult {
    Edited,
    NoChange,
    FallbackRequired,
}

async fn try_edit_screen(
    bot: &Bot,
    chat_id: ChatId,
    message_id: i32,
    text: &str,
    keyboard: InlineKeyboardMarkup,
) -> Result<ScreenRenderResult, teloxide::RequestError> {
    let request = bot
        .edit_message_text(chat_id, MessageId(message_id), text.to_owned())
        .reply_markup(keyboard);

    match request.await {
        Ok(_) => Ok(ScreenRenderResult::Edited),
        Err(error) if is_message_not_modified_error(&error) => Ok(ScreenRenderResult::NoChange),
        Err(error) if is_edit_fallback_error(&error) => Ok(ScreenRenderResult::FallbackRequired),
        Err(error) => Err(error),
    }
}

fn is_message_not_modified_error(error: &teloxide::RequestError) -> bool {
    // Only treat Telegram API errors as "not modified"; network / IO errors should propagate.
    let teloxide::RequestError::Api(api_error) = error else {
        return false;
    };
    api_error
        .to_string()
        .to_ascii_lowercase()
        .contains("message is not modified")
}

fn is_edit_fallback_error(error: &teloxide::RequestError) -> bool {
    // Only treat Telegram API errors as edit-fallback triggers; anything else should propagate.
    let teloxide::RequestError::Api(api_error) = error else {
        return false;
    };
    let text = api_error.to_string().to_ascii_lowercase();
    text.contains("message to edit not found")
        || text.contains("message can't be edited")
        || text.contains("message can not be edited")
        || text.contains("there is no text in the message to edit")
}
