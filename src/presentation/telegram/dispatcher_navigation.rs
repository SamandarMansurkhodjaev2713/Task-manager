use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::use_cases::collect_stats::StatsScope;
use crate::application::use_cases::list_tasks::TaskListScope;
use crate::domain::errors::AppError;
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::TaskListOrigin;
use crate::presentation::telegram::ui;

use super::dispatcher_transport::{send_error, send_fresh_screen, send_screen};
use super::TelegramRuntime;

pub(crate) async fn show_main_menu(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.clear(chat_id.0).await;
    state.task_interactions.clear(chat_id.0).await;
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::MainMenu,
        &ui::welcome_text(actor),
        ui::main_menu_keyboard(actor),
    )
    .await
}

pub(crate) async fn show_main_menu_fresh(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.clear(chat_id.0).await;
    state.task_interactions.clear(chat_id.0).await;
    send_fresh_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::MainMenu,
        &ui::welcome_text(actor),
        ui::main_menu_keyboard(actor),
    )
    .await
}

pub(crate) async fn show_help(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    actor: &User,
) -> Result<(), teloxide::RequestError> {
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::Help,
        &ui::help_text(),
        ui::main_menu_keyboard(actor),
    )
    .await
}

pub(crate) async fn show_settings(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    actor: &User,
) -> Result<(), teloxide::RequestError> {
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::Settings,
        &ui::settings_text(actor),
        ui::main_menu_keyboard(actor),
    )
    .await
}

pub(crate) async fn show_create_menu(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.clear(chat_id.0).await;
    state.task_interactions.clear(chat_id.0).await;
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::CreateMenu,
        &ui::create_menu_text(),
        ui::create_menu_keyboard(),
    )
    .await
}

pub(crate) async fn show_task_list(
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
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::TaskList(origin),
                &text,
                keyboard,
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

pub(crate) async fn show_stats(
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
                state,
                chat_id,
                ScreenDescriptor::Stats(scope),
                &ui::stats_text(title, &stats),
                ui::main_menu_keyboard(actor),
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

pub(crate) async fn sync_employees(
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
                state,
                chat_id,
                ScreenDescriptor::SyncEmployeesResult,
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
