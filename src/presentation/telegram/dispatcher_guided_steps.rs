use teloxide::types::ChatId;
use teloxide::Bot;

use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::drafts::{CreationSession, GuidedTaskDraft, GuidedTaskStep};
use crate::presentation::telegram::ui;
use crate::shared::constants::limits::MIN_TASK_DESCRIPTION_LENGTH;

use super::dispatcher_transport::send_screen;
use super::{
    TelegramRuntime, GUIDED_FALLBACK_NAME, GUIDED_SYNTHETIC_MESSAGE_ID,
    GUIDED_TEXT_REQUIRED_MESSAGE,
};

pub(crate) async fn handle_guided_message(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    draft: GuidedTaskDraft,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    let Some(text) = incoming_message.text_payload() else {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::GuidedStep(draft.step),
            GUIDED_TEXT_REQUIRED_MESSAGE,
            ui::main_menu_keyboard(&actor),
        )
        .await;
    };

    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::GuidedStep(draft.step),
            GUIDED_TEXT_REQUIRED_MESSAGE,
            ui::main_menu_keyboard(&actor),
        )
        .await;
    }

    match draft.step {
        GuidedTaskStep::Assignee => {
            handle_guided_assignee_step(bot, state, chat_id, trimmed_text).await
        }
        GuidedTaskStep::Description => {
            handle_guided_description_step(bot, state, chat_id, draft, trimmed_text).await
        }
        GuidedTaskStep::Deadline => {
            handle_guided_deadline_step(bot, state, chat_id, draft, trimmed_text).await
        }
        GuidedTaskStep::Confirm => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
                &ui::guided_confirmation_text(&draft),
                ui::guided_confirmation_keyboard(),
            )
            .await
        }
    }
}

pub(crate) async fn show_guided_confirmation(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: i64,
    mut draft: GuidedTaskDraft,
) -> Result<(), teloxide::RequestError> {
    draft.step = GuidedTaskStep::Confirm;
    state
        .creation_sessions
        .update_guided(chat_id, draft.clone())
        .await;
    send_screen(
        bot,
        state,
        ChatId(chat_id),
        ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
        &ui::guided_confirmation_text(&draft),
        ui::guided_confirmation_keyboard(),
    )
    .await
}

pub(crate) fn build_guided_message(
    chat_id: i64,
    actor: &User,
    draft: &GuidedTaskDraft,
    description: &str,
) -> IncomingMessage {
    let base_text = match draft.assignee.as_deref() {
        Some(assignee) => format!("{assignee}, {description}"),
        None => description.to_owned(),
    };
    let deadline_suffix = draft
        .deadline
        .as_ref()
        .map(|value| build_deadline_suffix(value))
        .unwrap_or_default();
    let text = format!("{base_text}{deadline_suffix}");

    IncomingMessage {
        message_id: GUIDED_SYNTHETIC_MESSAGE_ID,
        chat_id,
        sender_id: actor.telegram_id,
        sender_name: actor
            .full_name
            .clone()
            .unwrap_or_else(|| GUIDED_FALLBACK_NAME.to_owned()),
        sender_username: actor.telegram_username.clone(),
        content: MessageContent::Text { text },
        timestamp: chrono::Utc::now(),
        source_message_key_override: Some(format!(
            "telegram:guided:{chat_id}:{}",
            draft.submission_key
        )),
    }
}

async fn handle_guided_assignee_step(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    value: &str,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Guided(draft)) = state.creation_sessions.get(chat_id.0).await else {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::CreateMenu,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    let updated = update_guided_assignee(draft, value);
    state
        .creation_sessions
        .update_guided(chat_id.0, updated)
        .await;
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
        &ui::guided_description_prompt(),
        ui::create_menu_keyboard(),
    )
    .await
}

async fn handle_guided_description_step(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    draft: GuidedTaskDraft,
    value: &str,
) -> Result<(), teloxide::RequestError> {
    if let Some(validation_message) = validate_guided_description(value) {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
            validation_message,
            ui::create_menu_keyboard(),
        )
        .await;
    }

    let updated = update_guided_description(draft, value);
    state
        .creation_sessions
        .update_guided(chat_id.0, updated)
        .await;
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::GuidedStep(GuidedTaskStep::Deadline),
        &ui::guided_deadline_prompt(),
        ui::guided_deadline_keyboard(),
    )
    .await
}

async fn handle_guided_deadline_step(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    draft: GuidedTaskDraft,
    value: &str,
) -> Result<(), teloxide::RequestError> {
    let updated = update_guided_deadline(draft, value);
    show_guided_confirmation(bot, state, chat_id.0, updated).await
}

fn update_guided_assignee(mut draft: GuidedTaskDraft, value: &str) -> GuidedTaskDraft {
    draft.assignee = Some(value.to_owned());
    draft.step = GuidedTaskStep::Description;
    draft
}

fn update_guided_description(mut draft: GuidedTaskDraft, value: &str) -> GuidedTaskDraft {
    draft.description = Some(value.to_owned());
    draft.step = GuidedTaskStep::Deadline;
    draft
}

fn update_guided_deadline(mut draft: GuidedTaskDraft, value: &str) -> GuidedTaskDraft {
    draft.deadline = Some(value.to_owned());
    draft.step = GuidedTaskStep::Confirm;
    draft
}

fn build_deadline_suffix(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed
        .chars()
        .next()
        .is_some_and(|symbol| symbol.is_ascii_digit())
    {
        format!(" до {trimmed}")
    } else {
        format!(" {trimmed}")
    }
}

fn validate_guided_description(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    if trimmed.chars().count() < MIN_TASK_DESCRIPTION_LENGTH {
        return Some(
            "Описание пока слишком короткое. Напишите чуть конкретнее: что именно нужно сделать и какой результат ждём.",
        );
    }

    if trimmed.split_whitespace().count() < 2 {
        return Some(
            "Формулировка пока выглядит слишком короткой. Нужна одна нормальная рабочая фраза, а не одно слово.",
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::validate_guided_description;

    #[test]
    fn given_too_short_description_when_validating_then_returns_hint() {
        let validation = validate_guided_description("сделать");

        assert!(validation.is_some());
    }

    #[test]
    fn given_workable_description_when_validating_then_accepts_it() {
        let validation = validate_guided_description("подготовить чек-лист релиза");

        assert!(validation.is_none());
    }
}
