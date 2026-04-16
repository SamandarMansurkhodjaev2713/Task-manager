use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::dto::task_views::TaskCreationOutcome;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::drafts::{CreationSession, GuidedTaskStep};
use crate::presentation::telegram::ui;

use super::dispatcher_creation_outcomes::{
    keyboard_for_outcome, outcome_descriptor, should_clear_session,
};
use super::dispatcher_guided_steps::{
    build_guided_message, handle_guided_message, show_guided_confirmation,
};
use super::dispatcher_transport::{send_error, send_screen};
use super::dispatcher_voice::{handle_voice_creation_session_message, start_voice_create};
use super::{TelegramRuntime, GUIDED_DESCRIPTION_REQUIRED_MESSAGE};

pub(crate) async fn handle_creation_session_message(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    session: CreationSession,
) -> Result<(), teloxide::RequestError> {
    match session {
        CreationSession::QuickCapture => {
            if matches!(&incoming_message.content, MessageContent::Voice { .. }) {
                return start_voice_create(bot, state, incoming_message).await;
            }
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
        CreationSession::Voice(draft) => {
            handle_voice_creation_session_message(bot, state, incoming_message, draft).await
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
        state,
        chat_id,
        ScreenDescriptor::QuickCreate,
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
        state,
        chat_id,
        ScreenDescriptor::GuidedStep(GuidedTaskStep::Assignee),
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
            state,
            chat_id,
            ScreenDescriptor::CreateMenu,
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
        state,
        chat_id,
        ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
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
            state,
            chat_id,
            ScreenDescriptor::CreateMenu,
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
            state,
            chat_id,
            ScreenDescriptor::CreateMenu,
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
                state,
                chat_id,
                ScreenDescriptor::GuidedStep(GuidedTaskStep::Assignee),
                &ui::guided_assignee_prompt(),
                ui::guided_assignee_keyboard(),
            )
            .await
        }
        GuidedTaskStep::Description => {
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
        GuidedTaskStep::Deadline => {
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

pub(crate) async fn submit_guided_draft(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
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
    let Some(description) = draft
        .description
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    else {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
            GUIDED_DESCRIPTION_REQUIRED_MESSAGE,
            ui::guided_confirmation_keyboard(),
        )
        .await;
    };

    let synthetic_message = build_guided_message(chat_id.0, actor, &draft, description);
    match state.create_task_use_case.execute(synthetic_message).await {
        Ok(outcome @ TaskCreationOutcome::Created(_))
        | Ok(outcome @ TaskCreationOutcome::DuplicateFound(_)) => {
            state.creation_sessions.clear(chat_id.0).await;
            send_screen(
                bot,
                state,
                chat_id,
                outcome_descriptor(&outcome),
                &ui::task_creation_text(&outcome),
                ui::outcome_keyboard(&outcome),
            )
            .await
        }
        Ok(outcome @ TaskCreationOutcome::ClarificationRequired(_)) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
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
            send_screen(
                bot,
                state,
                chat_id,
                outcome_descriptor(&outcome),
                &ui::task_creation_text(&outcome),
                keyboard,
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

#[derive(Clone, Copy)]
pub(crate) enum SessionCompletion {
    Clear,
    KeepOnClarification,
}
