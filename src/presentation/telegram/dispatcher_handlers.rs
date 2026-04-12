use teloxide::prelude::Requester;
use teloxide::types::ChatId;
use teloxide::Bot;
use uuid::Uuid;

use crate::application::use_cases::collect_stats::StatsScope;
use crate::application::use_cases::list_tasks::TaskListScope;
use crate::domain::errors::AppError;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::task::TaskStatus;
use crate::domain::user::User;
use crate::presentation::telegram::callbacks::{TaskCardMode, TaskListOrigin, TelegramCallback};
use crate::presentation::telegram::commands::BotCommand;
use crate::presentation::telegram::ui;

use super::dispatcher_guided::{
    create_task_and_present, edit_guided_field, skip_guided_assignee, skip_guided_deadline,
    start_guided_create, start_quick_create, submit_guided_draft, SessionCompletion,
};
use super::dispatcher_interactions::{
    start_task_blocker_input, start_task_comment_input, start_task_reassign_input,
};
use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;
use super::RATE_LIMIT_MESSAGE;

pub(crate) async fn register_actor(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: &IncomingMessage,
) -> Result<Option<User>, teloxide::RequestError> {
    match state.register_user_use_case.execute(incoming_message).await {
        Ok(actor) => Ok(Some(actor)),
        Err(error) => {
            send_error(bot, incoming_message.chat_id, error).await?;
            Ok(None)
        }
    }
}

pub(crate) async fn check_rate_limit(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<bool, teloxide::RequestError> {
    let actor_key = u64::try_from(actor.telegram_id).unwrap_or_default();
    if state.rate_limiter.check(actor_key) {
        return Ok(true);
    }

    bot.send_message(chat_id, RATE_LIMIT_MESSAGE).await?;
    Ok(false)
}

pub(crate) async fn handle_command(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    command: BotCommand,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    state.task_interactions.clear(chat_id.0).await;

    match command {
        BotCommand::Start | BotCommand::Menu => show_main_menu(bot, state, &actor, chat_id).await,
        BotCommand::Help => show_help(bot, chat_id, &actor).await,
        BotCommand::NewTask { payload } => {
            handle_new_task_command(bot, state, incoming_message, chat_id, payload).await
        }
        BotCommand::MyTasks { cursor } => {
            show_task_list(
                bot,
                state,
                &actor,
                chat_id,
                TaskListOrigin::Assigned,
                cursor,
            )
            .await
        }
        BotCommand::CreatedTasks { cursor } => {
            show_task_list(bot, state, &actor, chat_id, TaskListOrigin::Created, cursor).await
        }
        BotCommand::TeamTasks { cursor } => {
            show_task_list(bot, state, &actor, chat_id, TaskListOrigin::Team, cursor).await
        }
        BotCommand::Status { task_uid } => {
            show_task_from_command(bot, state, &actor, chat_id, &task_uid).await
        }
        BotCommand::CancelTask { task_uid } => {
            execute_cancel_from_command(bot, state, &actor, chat_id, &task_uid).await
        }
        BotCommand::Stats => show_stats(bot, state, &actor, chat_id, StatsScope::Personal).await,
        BotCommand::TeamStats => show_stats(bot, state, &actor, chat_id, StatsScope::Team).await,
        BotCommand::Settings => show_settings(bot, chat_id, &actor).await,
        BotCommand::AdminSyncEmployees => sync_employees(bot, state, chat_id, &actor).await,
    }
}

pub(crate) async fn handle_callback_action(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    callback: TelegramCallback,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    state.task_interactions.clear(chat_id.0).await;

    match callback {
        TelegramCallback::MenuHome => show_main_menu(bot, state, &actor, chat_id).await,
        TelegramCallback::MenuHelp => show_help(bot, chat_id, &actor).await,
        TelegramCallback::MenuSettings => show_settings(bot, chat_id, &actor).await,
        TelegramCallback::MenuStats => {
            show_stats(bot, state, &actor, chat_id, StatsScope::Personal).await
        }
        TelegramCallback::MenuTeamStats => {
            show_stats(bot, state, &actor, chat_id, StatsScope::Team).await
        }
        TelegramCallback::MenuCreate => show_create_menu(bot, state, chat_id).await,
        TelegramCallback::MenuSyncEmployees => sync_employees(bot, state, chat_id, &actor).await,
        TelegramCallback::ListTasks { origin, cursor } => {
            show_task_list(bot, state, &actor, chat_id, origin, cursor).await
        }
        TelegramCallback::OpenTask {
            task_uid,
            origin,
            mode,
        } => show_task_details(bot, state, &actor, chat_id, task_uid, origin, mode).await,
        TelegramCallback::UpdateTaskStatus {
            task_uid,
            next_status,
            origin,
        } => update_task_status(bot, state, &actor, chat_id, task_uid, next_status, origin).await,
        TelegramCallback::ConfirmTaskCancel { task_uid, origin } => {
            confirm_task_cancel(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::ExecuteTaskCancel { task_uid, origin } => {
            update_task_status(
                bot,
                state,
                &actor,
                chat_id,
                task_uid,
                TaskStatus::Cancelled,
                origin,
            )
            .await
        }
        TelegramCallback::StartTaskCommentInput { task_uid, origin } => {
            start_task_comment_input(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::StartTaskBlockerInput { task_uid, origin } => {
            start_task_blocker_input(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::StartTaskReassignInput { task_uid, origin } => {
            start_task_reassign_input(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::StartQuickCreate => start_quick_create(bot, state, chat_id).await,
        TelegramCallback::StartGuidedCreate => start_guided_create(bot, state, chat_id).await,
        TelegramCallback::DraftSkipAssignee => skip_guided_assignee(bot, state, chat_id).await,
        TelegramCallback::DraftSkipDeadline => skip_guided_deadline(bot, state, chat_id).await,
        TelegramCallback::DraftSubmit => submit_guided_draft(bot, state, &actor, chat_id).await,
        TelegramCallback::DraftEdit { field } => {
            edit_guided_field(bot, state, chat_id, field).await
        }
    }
}

async fn show_main_menu(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.clear(chat_id.0).await;
    state.task_interactions.clear(chat_id.0).await;
    send_screen(
        bot,
        chat_id,
        &ui::welcome_text(actor),
        ui::main_menu_keyboard(actor),
    )
    .await
}

async fn show_help(bot: &Bot, chat_id: ChatId, actor: &User) -> Result<(), teloxide::RequestError> {
    send_screen(
        bot,
        chat_id,
        &ui::help_text(),
        ui::main_menu_keyboard(actor),
    )
    .await
}

async fn show_settings(
    bot: &Bot,
    chat_id: ChatId,
    actor: &User,
) -> Result<(), teloxide::RequestError> {
    send_screen(
        bot,
        chat_id,
        &ui::settings_text(actor),
        ui::main_menu_keyboard(actor),
    )
    .await
}

async fn show_create_menu(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.clear(chat_id.0).await;
    state.task_interactions.clear(chat_id.0).await;
    send_screen(
        bot,
        chat_id,
        &ui::create_menu_text(),
        ui::create_menu_keyboard(),
    )
    .await
}

async fn handle_new_task_command(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    chat_id: ChatId,
    payload: Option<String>,
) -> Result<(), teloxide::RequestError> {
    match payload {
        Some(payload) => {
            let synthetic_message = IncomingMessage {
                content: MessageContent::Text { text: payload },
                ..incoming_message
            };
            create_task_and_present(
                bot,
                state,
                chat_id,
                synthetic_message,
                SessionCompletion::Clear,
            )
            .await
        }
        None => show_create_menu(bot, state, chat_id).await,
    }
}

async fn show_task_list(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    origin: TaskListOrigin,
    cursor: Option<String>,
) -> Result<(), teloxide::RequestError> {
    match state
        .list_tasks_use_case
        .execute(actor, list_scope(origin), cursor, None)
        .await
    {
        Ok(page) => {
            let (title, subtitle) = ui::list_header(origin);
            let text = ui::list_text(title, subtitle, &page);
            let keyboard = ui::task_list_keyboard(origin, &page);
            send_screen(bot, chat_id, &text, keyboard).await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

async fn show_task_from_command(
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
                chat_id,
                "Неверный формат ID задачи. Используйте UUID из карточки или списка задач.",
                ui::main_menu_keyboard(actor),
            )
            .await
        }
    }
}

async fn execute_cancel_from_command(
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
            update_task_status(
                bot,
                state,
                actor,
                chat_id,
                task_uid,
                TaskStatus::Cancelled,
                TaskListOrigin::Created,
            )
            .await
        }
        Err(_) => {
            send_screen(
                bot,
                chat_id,
                "Неверный формат ID задачи. Используйте UUID из карточки или списка задач.",
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
    match state
        .get_task_status_use_case
        .execute(actor, task_uid)
        .await
    {
        Ok(details) => {
            let text = ui::task_detail_text(&details, mode);
            let keyboard = ui::task_detail_keyboard(&details, origin, mode);
            send_screen(bot, chat_id, &text, keyboard).await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

async fn confirm_task_cancel(
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
            send_screen(bot, chat_id, &text, keyboard).await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

async fn update_task_status(
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
            bot.send_message(chat_id, summary.message).await?;
            show_task_details(
                bot,
                state,
                actor,
                chat_id,
                task_uid,
                origin,
                TaskCardMode::Compact,
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

async fn show_stats(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    scope: StatsScope,
) -> Result<(), teloxide::RequestError> {
    match state.collect_stats_use_case.execute(actor, scope).await {
        Ok(stats) => {
            let title = match scope {
                StatsScope::Personal => "📊 Моя статистика",
                StatsScope::Team => "📈 Статистика команды",
            };
            send_screen(
                bot,
                chat_id,
                &ui::stats_text(title, &stats),
                ui::main_menu_keyboard(actor),
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

async fn sync_employees(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    actor: &User,
) -> Result<(), teloxide::RequestError> {
    if !actor.role.is_admin() {
        return send_error(
            bot,
            chat_id.0,
            AppError::unauthorized(
                "Only admins can trigger employee sync",
                serde_json::json!({}),
            ),
        )
        .await;
    }

    match state.sync_employees_use_case.execute().await {
        Ok(count) => {
            send_screen(
                bot,
                chat_id,
                &ui::synced_text(count),
                ui::main_menu_keyboard(actor),
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

fn list_scope(origin: TaskListOrigin) -> TaskListScope {
    match origin {
        TaskListOrigin::Assigned => TaskListScope::AssignedToMe,
        TaskListOrigin::Created => TaskListScope::CreatedByMe,
        TaskListOrigin::Team => TaskListScope::Team,
        TaskListOrigin::Focus => TaskListScope::Focus,
        TaskListOrigin::ManagerInbox => TaskListScope::ManagerInbox,
    }
}
