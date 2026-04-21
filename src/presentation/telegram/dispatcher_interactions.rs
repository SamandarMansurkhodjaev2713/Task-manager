use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::dto::task_views::TaskCreationOutcome;
use crate::application::use_cases::reassign_task::ReassignTaskOutcome;
use crate::domain::message::IncomingMessage;
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::TaskCardMode;
use crate::presentation::telegram::interactions::{TaskInteractionKind, TaskInteractionSession};
use crate::presentation::telegram::ui;

use super::dispatcher_handlers::{show_task_details_with_notice, TaskScreenContext};
use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

const TASK_INTERACTION_TEXT_REQUIRED_MESSAGE: &str =
    "Нужен один короткий текст сообщением. Напишите по делу, и я сразу обновлю карточку.";
const COMMENT_SAVED_NOTICE: &str = "💬 Комментарий добавлен.";
const BLOCKER_SAVED_NOTICE: &str = "🚧 Блокер сохранён и показан в карточке.";

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
                Ok(_) => {
                    state.task_interactions.clear(chat_id.0).await;
                    show_task_details_with_notice(
                        bot,
                        state,
                        &actor,
                        TaskScreenContext {
                            chat_id,
                            task_uid: session.task_uid,
                            origin: session.origin,
                            mode: TaskCardMode::Compact,
                        },
                        Some(COMMENT_SAVED_NOTICE),
                    )
                    .await
                }
                Err(error) => send_error(bot, state, chat_id.0, error).await,
            }
        }
        TaskInteractionKind::Blocker => {
            match state
                .report_task_blocker_use_case
                .execute(&actor, session.task_uid, text)
                .await
            {
                Ok(_) => {
                    state.task_interactions.clear(chat_id.0).await;
                    show_task_details_with_notice(
                        bot,
                        state,
                        &actor,
                        TaskScreenContext {
                            chat_id,
                            task_uid: session.task_uid,
                            origin: session.origin,
                            mode: TaskCardMode::Compact,
                        },
                        Some(BLOCKER_SAVED_NOTICE),
                    )
                    .await
                }
                Err(error) => send_error(bot, state, chat_id.0, error).await,
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
                    state.assignee_selections.clear(chat_id.0).await;
                    show_task_details_with_notice(
                        bot,
                        state,
                        &actor,
                        TaskScreenContext {
                            chat_id,
                            task_uid: session.task_uid,
                            origin: session.origin,
                            mode: TaskCardMode::Compact,
                        },
                        Some(&summary.message),
                    )
                    .await
                }
                Ok(ReassignTaskOutcome::ClarificationRequired(request)) => {
                    state
                        .assignee_selections
                        .set_reassign(
                            chat_id.0,
                            session.task_uid,
                            session.origin,
                            text.to_owned(),
                            clarification_candidate_ids(&request),
                        )
                        .await;
                    send_screen(
                        bot,
                        state,
                        chat_id,
                        ScreenDescriptor::TaskInteractionPrompt {
                            task_uid: session.task_uid,
                            origin: session.origin,
                            kind: session.kind,
                        },
                        &ui::task_creation_text(&TaskCreationOutcome::ClarificationRequired(
                            request.clone(),
                        )),
                        ui::clarification_keyboard(&request),
                    )
                    .await
                }
                Err(error) => send_error(bot, state, chat_id.0, error).await,
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
                state,
                chat_id,
                ScreenDescriptor::TaskInteractionPrompt {
                    task_uid,
                    origin,
                    kind,
                },
                &text,
                ui::task_detail_keyboard(&details, origin, TaskCardMode::Compact),
            )
            .await
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
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
                state,
                chat_id,
                ScreenDescriptor::TaskInteractionPrompt {
                    task_uid: session.task_uid,
                    origin: session.origin,
                    kind: session.kind,
                },
                &text,
                ui::task_detail_keyboard(&details, session.origin, TaskCardMode::Compact),
            )
            .await
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
    }
}

fn clarification_candidate_ids(
    request: &crate::application::dto::task_views::ClarificationRequest,
) -> Vec<i64> {
    request
        .candidates
        .iter()
        .filter_map(|candidate| candidate.employee_id)
        .collect()
}
