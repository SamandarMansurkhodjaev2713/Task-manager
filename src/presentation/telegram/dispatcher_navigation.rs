use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::policies::role_authorization::RoleAuthorizationPolicy;
use crate::application::use_cases::collect_stats::StatsScope;
use crate::application::use_cases::list_tasks::TaskListScope;
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::{HelpSection, TaskListOrigin};
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
    state.assignee_selections.clear(chat_id.0).await;
    state.registration_links.clear(chat_id.0).await;
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
    state.assignee_selections.clear(chat_id.0).await;
    state.registration_links.clear(chat_id.0).await;
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

/// Корневой экран `/help`: краткое приветствие + клавиатура подразделов,
/// фильтрованная под роль актора.
///
/// Сам текст overview не перечисляет подразделы — это сделают кнопки.  Так мы
/// избегаем дублирования и упрощаем поддержку: добавление нового подраздела
/// не требует править текст приветствия.
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
        &ui::help_overview_text(actor),
        ui::help_overview_keyboard(actor),
    )
    .await
}

/// Открывает конкретный подраздел справки.  Вторая линия защиты: даже если
/// keyboard каким-то образом отрендерил кнопку для запретного раздела (или
/// callback пришёл от старого сообщения после демоута роли), мы перерисовываем
/// overview с понятным сообщением, не падая в `unauthorized`.
pub(crate) async fn show_help_section(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    actor: &User,
    section: HelpSection,
) -> Result<(), teloxide::RequestError> {
    if !section.is_visible_to(actor.role) {
        // Логируем для security-аудита: попытка доступа к более привилегированному
        // разделу справки — обычно симптом стейл-callback'а после демоута, реже —
        // ручной подмены payload.
        tracing::warn!(
            actor_id = ?actor.id,
            actor_role = ?actor.role,
            section = ?section,
            "help section access denied: role does not satisfy visibility predicate",
        );
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::Help,
            &ui::help_section_forbidden_text(section),
            ui::help_overview_keyboard(actor),
        )
        .await;
    }

    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::HelpSection { section },
        ui::help_section_text(section),
        ui::help_section_keyboard(),
    )
    .await
}

pub(crate) async fn show_settings(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    actor: &User,
) -> Result<(), teloxide::RequestError> {
    // Profile analytics (Phase 12): fetch the actor's personal task stats
    // on a best-effort basis so we can inline them into the profile screen.
    // Analytics MUST NOT block rendering the profile itself — if the stats
    // call fails, we fall back to the plain profile text and log the
    // failure for observability.
    let stats = match state
        .collect_stats_use_case
        .execute(
            actor,
            crate::application::use_cases::collect_stats::StatsScope::Personal,
        )
        .await
    {
        Ok(view) => Some(view),
        Err(error) => {
            tracing::warn!(
                error_code = error.code(),
                error = %error,
                "profile analytics: personal stats unavailable, rendering profile without them",
            );
            None
        }
    };

    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::Settings,
        &ui::settings_text_with_stats(actor, stats.as_ref()),
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
    state.assignee_selections.clear(chat_id.0).await;
    state.registration_links.clear(chat_id.0).await;
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
        Err(error) => send_error(bot, state, chat_id.0, error).await,
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
        Err(error) => send_error(bot, state, chat_id.0, error).await,
    }
}

pub(crate) async fn sync_employees(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    actor: &User,
) -> Result<(), teloxide::RequestError> {
    if let Err(error) = RoleAuthorizationPolicy::ensure_can_sync_employees(actor) {
        return send_error(bot, state, chat_id.0, error).await;
    }

    match state.sync_employees_use_case.execute().await {
        Ok(count) => {
            if let Err(error) = state
                .register_user_use_case
                .reconcile_existing_directory_links()
                .await
            {
                tracing::warn!(
                    code = error.code(),
                    "employee sync completed but user link reconciliation failed"
                );
            }
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
        Err(error) => send_error(bot, state, chat_id.0, error).await,
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
