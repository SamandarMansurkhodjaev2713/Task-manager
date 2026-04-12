use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::dto::task_views::TaskCreationOutcome;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::presentation::telegram::drafts::{CreationSession, GuidedTaskDraft, GuidedTaskStep};
use crate::presentation::telegram::ui;

use super::dispatcher_transport::{send_error, send_screen};
use super::{
    TelegramRuntime, GUIDED_DESCRIPTION_REQUIRED_MESSAGE, GUIDED_FALLBACK_NAME,
    GUIDED_SYNTHETIC_MESSAGE_ID, GUIDED_TEXT_REQUIRED_MESSAGE,
};

pub(crate) async fn handle_creation_session_message(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    session: CreationSession,
) -> Result<(), teloxide::RequestError> {
    match session {
        CreationSession::QuickCapture => {
            create_task_and_present(
                bot,
                state,
                ChatId(incoming_message.chat_id),
                incoming_message,
                SessionCompletion::KeepOnClarification,
            )
            .await
        }
        CreationSession::Guided(draft) => {
            handle_guided_message(bot, state, incoming_message, actor, draft).await
        }
    }
}

pub(crate) async fn start_quick_create(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.set_quick_capture(chat_id.0).await;
    send_screen(
        bot,
        chat_id,
        &ui::quick_create_prompt(),
        ui::create_menu_keyboard(),
    )
    .await
}

pub(crate) async fn start_guided_create(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.set_guided(chat_id.0).await;
    send_screen(
        bot,
        chat_id,
        &ui::guided_assignee_prompt(),
        ui::guided_assignee_keyboard(),
    )
    .await
}

pub(crate) async fn skip_guided_assignee(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Guided(mut draft)) = state.creation_sessions.get(chat_id.0).await
    else {
        return send_screen(
            bot,
            chat_id,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    draft.assignee = None;
    draft.step = GuidedTaskStep::Description;
    state
        .creation_sessions
        .update_guided(chat_id.0, draft)
        .await;
    send_screen(
        bot,
        chat_id,
        &ui::guided_description_prompt(),
        ui::create_menu_keyboard(),
    )
    .await
}

pub(crate) async fn skip_guided_deadline(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Guided(mut draft)) = state.creation_sessions.get(chat_id.0).await
    else {
        return send_screen(
            bot,
            chat_id,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    draft.deadline = None;
    draft.step = GuidedTaskStep::Confirm;
    show_guided_confirmation(bot, state, chat_id.0, draft).await
}

pub(crate) async fn edit_guided_field(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    field: crate::presentation::telegram::callbacks::DraftEditField,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Guided(mut draft)) = state.creation_sessions.get(chat_id.0).await
    else {
        return send_screen(
            bot,
            chat_id,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    draft.edit_field(field);
    state
        .creation_sessions
        .update_guided(chat_id.0, draft.clone())
        .await;

    match draft.step {
        GuidedTaskStep::Assignee => {
            send_screen(
                bot,
                chat_id,
                &ui::guided_assignee_prompt(),
                ui::guided_assignee_keyboard(),
            )
            .await
        }
        GuidedTaskStep::Description => {
            send_screen(
                bot,
                chat_id,
                &ui::guided_description_prompt(),
                ui::create_menu_keyboard(),
            )
            .await
        }
        GuidedTaskStep::Deadline => {
            send_screen(
                bot,
                chat_id,
                &ui::guided_deadline_prompt(),
                ui::guided_deadline_keyboard(),
            )
            .await
        }
        GuidedTaskStep::Confirm => {
            send_screen(
                bot,
                chat_id,
                &ui::guided_confirmation_text(&draft),
                ui::guided_confirmation_keyboard(),
            )
            .await
        }
    }
}

pub(crate) async fn submit_guided_draft(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Guided(draft)) = state.creation_sessions.get(chat_id.0).await else {
        return send_screen(
            bot,
            chat_id,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };
    let Some(description) = draft
        .description
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    else {
        return send_screen(
            bot,
            chat_id,
            GUIDED_DESCRIPTION_REQUIRED_MESSAGE,
            ui::guided_confirmation_keyboard(),
        )
        .await;
    };

    let synthetic_message = build_guided_message(chat_id.0, actor, &draft, description);
    match state.create_task_use_case.execute(synthetic_message).await {
        Ok(TaskCreationOutcome::Created(outcome)) => {
            state.creation_sessions.clear(chat_id.0).await;
            let created = TaskCreationOutcome::Created(outcome);
            send_screen(
                bot,
                chat_id,
                &ui::task_creation_text(&created),
                ui::outcome_keyboard(&created),
            )
            .await
        }
        Ok(outcome @ TaskCreationOutcome::ClarificationRequired(_)) => {
            send_screen(
                bot,
                chat_id,
                &ui::task_creation_text(&outcome),
                ui::guided_confirmation_keyboard(),
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

pub(crate) async fn create_task_and_present(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    incoming_message: IncomingMessage,
    session_completion: SessionCompletion,
) -> Result<(), teloxide::RequestError> {
    match state.create_task_use_case.execute(incoming_message).await {
        Ok(outcome) => {
            if should_clear_session(&outcome, session_completion) {
                state.creation_sessions.clear(chat_id.0).await;
            }
            let keyboard = keyboard_for_outcome(&outcome, session_completion);
            send_screen(bot, chat_id, &ui::task_creation_text(&outcome), keyboard).await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

async fn handle_guided_message(
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
            chat_id,
            GUIDED_TEXT_REQUIRED_MESSAGE,
            ui::main_menu_keyboard(&actor),
        )
        .await;
    };

    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return send_screen(
            bot,
            chat_id,
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
                chat_id,
                &ui::guided_confirmation_text(&draft),
                ui::guided_confirmation_keyboard(),
            )
            .await
        }
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
            chat_id,
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
        chat_id,
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
    let updated = update_guided_description(draft, value);
    state
        .creation_sessions
        .update_guided(chat_id.0, updated)
        .await;
    send_screen(
        bot,
        chat_id,
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

async fn show_guided_confirmation(
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
        ChatId(chat_id),
        &ui::guided_confirmation_text(&draft),
        ui::guided_confirmation_keyboard(),
    )
    .await
}

fn keyboard_for_outcome(
    outcome: &TaskCreationOutcome,
    session_completion: SessionCompletion,
) -> teloxide::types::InlineKeyboardMarkup {
    match session_completion {
        SessionCompletion::KeepOnClarification
            if matches!(outcome, TaskCreationOutcome::ClarificationRequired(_)) =>
        {
            ui::create_menu_keyboard()
        }
        _ => ui::outcome_keyboard(outcome),
    }
}

fn should_clear_session(outcome: &TaskCreationOutcome, completion: SessionCompletion) -> bool {
    match completion {
        SessionCompletion::Clear => true,
        SessionCompletion::KeepOnClarification => {
            matches!(outcome, TaskCreationOutcome::Created(_))
        }
    }
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

fn build_guided_message(
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

#[derive(Clone, Copy)]
pub(crate) enum SessionCompletion {
    Clear,
    KeepOnClarification,
}
