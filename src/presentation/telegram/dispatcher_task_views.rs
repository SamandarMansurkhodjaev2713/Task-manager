use teloxide::types::ChatId;
use teloxide::Bot;
use uuid::Uuid;

use crate::domain::task::TaskStatus;
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::{TaskCardMode, TaskListOrigin};
use crate::presentation::telegram::ui;

use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

const INVALID_TASK_REFERENCE_MESSAGE: &str =
    "Не удалось распознать задачу. Используйте код вида T-0001 или откройте карточку из списка.";

#[derive(Clone, Copy)]
pub(crate) struct TaskScreenContext {
    pub chat_id: ChatId,
    pub task_uid: Uuid,
    pub origin: TaskListOrigin,
    pub mode: TaskCardMode,
}

pub(crate) async fn show_task_from_command(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: &str,
) -> Result<(), teloxide::RequestError> {
    match state
        .get_task_status_use_case
        .resolve_task_uid(task_uid)
        .await
    {
        Ok(task_uid) => {
            show_task_details(
                bot,
                state,
                actor,
                chat_id,
                task_uid,
                TaskListOrigin::Created,
                TaskCardMode::Compact,
            )
            .await
        }
        Err(_) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::Unknown,
                INVALID_TASK_REFERENCE_MESSAGE,
                ui::main_menu_keyboard(actor),
            )
            .await
        }
    }
}

pub(crate) async fn execute_cancel_from_command(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: &str,
) -> Result<(), teloxide::RequestError> {
    match state
        .get_task_status_use_case
        .resolve_task_uid(task_uid)
        .await
    {
        Ok(task_uid) => {
            // Show confirmation screen — same flow as the inline button to avoid
            // accidental cancellations from a command typo.
            confirm_task_cancel(
                bot,
                state,
                actor,
                chat_id,
                task_uid,
                TaskListOrigin::Created,
            )
            .await
        }
        Err(_) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::Unknown,
                INVALID_TASK_REFERENCE_MESSAGE,
                ui::main_menu_keyboard(actor),
            )
            .await
        }
    }
}

pub(crate) async fn show_task_details(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: Uuid,
    origin: TaskListOrigin,
    mode: TaskCardMode,
) -> Result<(), teloxide::RequestError> {
    show_task_details_with_notice(
        bot,
        state,
        actor,
        TaskScreenContext {
            chat_id,
            task_uid,
            origin,
            mode,
        },
        None,
    )
    .await
}

pub(crate) async fn show_task_details_with_notice(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    context: TaskScreenContext,
    notice: Option<&str>,
) -> Result<(), teloxide::RequestError> {
    match state
        .get_task_status_use_case
        .execute(actor, context.task_uid)
        .await
    {
        Ok(details) => {
            let text = ui::task_detail_text(&details, context.mode, notice);
            let keyboard = ui::task_detail_keyboard(&details, context.origin, context.mode);
            send_screen(
                bot,
                state,
                context.chat_id,
                ScreenDescriptor::TaskDetail {
                    task_uid: context.task_uid,
                    mode: context.mode,
                    origin: context.origin,
                },
                &text,
                keyboard,
            )
            .await
        }
        Err(error) => send_error(bot, state, context.chat_id.0, error).await,
    }
}

pub(crate) async fn confirm_task_cancel(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: Uuid,
    origin: TaskListOrigin,
) -> Result<(), teloxide::RequestError> {
    match state
        .get_task_status_use_case
        .execute(actor, task_uid)
        .await
    {
        Ok(details) => {
            let text = ui::cancel_confirmation_text(&details);
            let keyboard = ui::cancel_confirmation_keyboard(task_uid, origin);
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::CancelConfirmation { task_uid, origin },
                &text,
                keyboard,
            )
            .await
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
    }
}

pub(crate) async fn update_task_status(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: Uuid,
    next_status: TaskStatus,
    origin: TaskListOrigin,
) -> Result<(), teloxide::RequestError> {
    match state
        .update_task_status_use_case
        .execute(actor, task_uid, next_status)
        .await
    {
        Ok(summary) => {
            show_task_details_with_notice(
                bot,
                state,
                actor,
                TaskScreenContext {
                    chat_id,
                    task_uid,
                    origin,
                    mode: TaskCardMode::Compact,
                },
                Some(&summary.message),
            )
            .await
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
    }
}

pub(crate) async fn show_delivery_help(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    task_uid: Uuid,
    origin: TaskListOrigin,
) -> Result<(), teloxide::RequestError> {
    match state
        .get_task_status_use_case
        .execute(actor, task_uid)
        .await
    {
        Ok(details) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::DeliveryHelp { task_uid, origin },
                &ui::delivery_help_text(&details),
                ui::delivery_help_keyboard(task_uid, origin),
            )
            .await
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
    }
}
