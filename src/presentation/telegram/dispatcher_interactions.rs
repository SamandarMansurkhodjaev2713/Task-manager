use teloxide::prelude::Requester;
use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::use_cases::reassign_task::ReassignTaskOutcome;
use crate::domain::message::IncomingMessage;
use crate::domain::user::User;
use crate::presentation::telegram::callbacks::TaskCardMode;
use crate::presentation::telegram::interactions::{TaskInteractionKind, TaskInteractionSession};
use crate::presentation::telegram::ui;

use super::dispatcher_handlers::show_task_details;
use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

const TASK_INTERACTION_TEXT_REQUIRED_MESSAGE: &str =
    "Здесь нужен текст одним сообщением. Напишите коротко и по делу.";

pub(crate) async fn start_task_comment_input(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: uuid::Uuid,
    origin: crate::presentation::telegram::callbacks::TaskListOrigin,
) -> Result<(), teloxide::RequestError> {
    start_task_interaction(
        bot,
        state,
        actor,
        chat_id,
        task_uid,
        origin,
        TaskInteractionKind::Comment,
    )
    .await
}

pub(crate) async fn start_task_blocker_input(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: uuid::Uuid,
    origin: crate::presentation::telegram::callbacks::TaskListOrigin,
) -> Result<(), teloxide::RequestError> {
    start_task_interaction(
        bot,
        state,
        actor,
        chat_id,
        task_uid,
        origin,
        TaskInteractionKind::Blocker,
    )
    .await
}

pub(crate) async fn start_task_reassign_input(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: uuid::Uuid,
    origin: crate::presentation::telegram::callbacks::TaskListOrigin,
) -> Result<(), teloxide::RequestError> {
    start_task_interaction(
        bot,
        state,
        actor,
        chat_id,
        task_uid,
        origin,
        TaskInteractionKind::Reassign,
    )
    .await
}

pub(crate) async fn handle_task_interaction_message(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    session: TaskInteractionSession,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    let Some(text) = incoming_message.text_payload().map(str::trim) else {
        return show_prompt_again(
            bot,
            state,
            &actor,
            chat_id,
            session,
            TASK_INTERACTION_TEXT_REQUIRED_MESSAGE,
        )
        .await;
    };
    if text.is_empty() {
        return show_prompt_again(
            bot,
            state,
            &actor,
            chat_id,
            session,
            TASK_INTERACTION_TEXT_REQUIRED_MESSAGE,
        )
        .await;
    }

    match session.kind {
        TaskInteractionKind::Comment => {
            match state
                .add_task_comment_use_case
                .execute(&actor, session.task_uid, text)
                .await
            {
                Ok(message) => {
                    state.task_interactions.clear(chat_id.0).await;
                    bot.send_message(chat_id, message).await?;
                    show_task_details(
                        bot,
                        state,
                        &actor,
                        chat_id,
                        session.task_uid,
                        session.origin,
                        TaskCardMode::Compact,
                    )
                    .await
                }
                Err(error) => send_error(bot, chat_id.0, error).await,
            }
        }
        TaskInteractionKind::Blocker => {
            match state
                .report_task_blocker_use_case
                .execute(&actor, session.task_uid, text)
                .await
            {
                Ok(summary) => {
                    state.task_interactions.clear(chat_id.0).await;
                    bot.send_message(chat_id, summary.message).await?;
                    show_task_details(
                        bot,
                        state,
                        &actor,
                        chat_id,
                        session.task_uid,
                        session.origin,
                        TaskCardMode::Compact,
                    )
                    .await
                }
                Err(error) => send_error(bot, chat_id.0, error).await,
            }
        }
        TaskInteractionKind::Reassign => {
            match state
                .reassign_task_use_case
                .execute(&actor, session.task_uid, text)
                .await
            {
                Ok(ReassignTaskOutcome::Reassigned(summary)) => {
                    state.task_interactions.clear(chat_id.0).await;
                    bot.send_message(chat_id, summary.message).await?;
                    show_task_details(
                        bot,
                        state,
                        &actor,
                        chat_id,
                        session.task_uid,
                        session.origin,
                        TaskCardMode::Compact,
                    )
                    .await
                }
                Ok(ReassignTaskOutcome::ClarificationRequired(request)) => {
                    let text = format!(
                        "ℹ️ {}\n\nПопробуйте ещё раз одним сообщением.",
                        request.message
                    );
                    show_prompt_again(bot, state, &actor, chat_id, session, &text).await
                }
                Err(error) => send_error(bot, chat_id.0, error).await,
            }
        }
    }
}

async fn start_task_interaction(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: uuid::Uuid,
    origin: crate::presentation::telegram::callbacks::TaskListOrigin,
    kind: TaskInteractionKind,
) -> Result<(), teloxide::RequestError> {
    match state
        .get_task_status_use_case
        .execute(actor, task_uid)
        .await
    {
        Ok(details) => {
            state
                .task_interactions
                .set(
                    chat_id.0,
                    TaskInteractionSession {
                        task_uid,
                        origin,
                        kind,
                    },
                )
                .await;

            let text = match kind {
                TaskInteractionKind::Comment => ui::task_comment_prompt(&details),
                TaskInteractionKind::Blocker => ui::task_blocker_prompt(&details),
                TaskInteractionKind::Reassign => ui::task_reassign_prompt(&details),
            };
            send_screen(
                bot,
                chat_id,
                &text,
                ui::task_detail_keyboard(&details, origin, TaskCardMode::Compact),
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

async fn show_prompt_again(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    session: TaskInteractionSession,
    prefix_message: &str,
) -> Result<(), teloxide::RequestError> {
    match state
        .get_task_status_use_case
        .execute(actor, session.task_uid)
        .await
    {
        Ok(details) => {
            let prompt = match session.kind {
                TaskInteractionKind::Comment => ui::task_comment_prompt(&details),
                TaskInteractionKind::Blocker => ui::task_blocker_prompt(&details),
                TaskInteractionKind::Reassign => ui::task_reassign_prompt(&details),
            };
            let text = format!("{prefix_message}\n\n{prompt}");
            send_screen(
                bot,
                chat_id,
                &text,
                ui::task_detail_keyboard(&details, session.origin, TaskCardMode::Compact),
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}
