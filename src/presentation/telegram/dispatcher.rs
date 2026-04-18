use std::sync::Arc;

use teloxide::dispatching::UpdateFilterExt;
use teloxide::dptree;
use teloxide::prelude::{CallbackQuery, Dispatcher, Message, Update};
use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::use_cases::add_task_comment::AddTaskCommentUseCase;
use crate::application::use_cases::collect_stats::CollectStatsUseCase;
use crate::application::use_cases::create_task_from_message::CreateTaskFromMessageUseCase;
use crate::application::use_cases::get_task_status::GetTaskStatusUseCase;
use crate::application::use_cases::list_tasks::ListTasksUseCase;
use crate::application::use_cases::reassign_task::ReassignTaskUseCase;
use crate::application::use_cases::register_user::RegisterUserUseCase;
use crate::application::use_cases::report_task_blocker::ReportTaskBlockerUseCase;
use crate::application::use_cases::sync_employees::SyncEmployeesUseCase;
use crate::application::use_cases::update_task_status::UpdateTaskStatusUseCase;
use crate::domain::errors::AppError;
use crate::infrastructure::telegram::bot_gateway::TeloxideNotifier;
use crate::presentation::telegram::active_screens::ActiveScreenStore;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::assignee_selections::PendingAssigneeSelectionStore;
use crate::presentation::telegram::callbacks::TelegramCallback;
use crate::presentation::telegram::commands::parse_command;
use crate::presentation::telegram::drafts::CreationSessionStore;
use crate::presentation::telegram::interactions::TaskInteractionSessionStore;
use crate::presentation::telegram::rate_limiter::TelegramRateLimiter;
use crate::presentation::telegram::registration_links::PendingRegistrationLinkStore;

use self::dispatcher_guided::{
    create_task_and_present, handle_creation_session_message, SessionCompletion,
};
use self::dispatcher_handlers::{
    check_rate_limit, handle_callback_action, handle_command, register_actor,
};
use self::dispatcher_interactions::handle_task_interaction_message;
use self::dispatcher_registration::handle_registration_link_callback;
use self::dispatcher_transport::{
    answer_callback, callback_to_incoming_message, to_incoming_message,
};
use self::dispatcher_voice::VoiceCreateCoordinator;

#[path = "dispatcher_assignee_clarifications.rs"]
mod dispatcher_assignee_clarifications;
#[path = "dispatcher_creation_outcomes.rs"]
mod dispatcher_creation_outcomes;
#[path = "dispatcher_guided.rs"]
mod dispatcher_guided;
#[path = "dispatcher_guided_steps.rs"]
mod dispatcher_guided_steps;
#[path = "dispatcher_handlers.rs"]
mod dispatcher_handlers;
#[path = "dispatcher_interactions.rs"]
mod dispatcher_interactions;
#[path = "dispatcher_navigation.rs"]
mod dispatcher_navigation;
#[path = "dispatcher_registration.rs"]
mod dispatcher_registration;
#[path = "dispatcher_task_views.rs"]
mod dispatcher_task_views;
#[path = "dispatcher_transport.rs"]
mod dispatcher_transport;
#[path = "dispatcher_voice.rs"]
mod dispatcher_voice;

pub(crate) const CALLBACK_OK_TEXT: &str = "Готово";
pub(crate) const STALE_MUTATION_CALLBACK_TEXT: &str =
    "Этот экран уже устарел. Откройте актуальную карточку или список.";
pub(crate) const STALE_NAVIGATION_CALLBACK_TEXT: &str = "Открываю актуальный экран.";
pub(crate) const RATE_LIMIT_MESSAGE: &str =
    "Слишком много запросов подряд. Подождите немного и попробуйте снова.";
pub(crate) const GUIDED_TEXT_REQUIRED_MESSAGE: &str =
    "На этом шаге нужен текст. Для голосовых удобнее использовать быстрый режим.";
pub(crate) const GUIDED_DESCRIPTION_REQUIRED_MESSAGE: &str =
    "Сначала добавьте описание задачи, потом я смогу её создать.";
pub(crate) const GUIDED_SYNTHETIC_MESSAGE_ID: i32 = 0;
pub(crate) const GUIDED_FALLBACK_NAME: &str = "Пользователь";

#[derive(Clone)]
pub struct TelegramRuntime {
    pub notifier: TeloxideNotifier,
    pub rate_limiter: TelegramRateLimiter,
    pub active_screens: ActiveScreenStore,
    pub assignee_selections: PendingAssigneeSelectionStore,
    pub registration_links: PendingRegistrationLinkStore,
    pub creation_sessions: CreationSessionStore,
    pub task_interactions: TaskInteractionSessionStore,
    pub register_user_use_case: Arc<RegisterUserUseCase>,
    pub create_task_use_case: Arc<CreateTaskFromMessageUseCase>,
    pub list_tasks_use_case: Arc<ListTasksUseCase>,
    pub get_task_status_use_case: Arc<GetTaskStatusUseCase>,
    pub update_task_status_use_case: Arc<UpdateTaskStatusUseCase>,
    pub add_task_comment_use_case: Arc<AddTaskCommentUseCase>,
    pub report_task_blocker_use_case: Arc<ReportTaskBlockerUseCase>,
    pub reassign_task_use_case: Arc<ReassignTaskUseCase>,
    pub collect_stats_use_case: Arc<CollectStatsUseCase>,
    pub sync_employees_use_case: Arc<SyncEmployeesUseCase>,
}

pub async fn run_telegram_dispatcher(runtime: TelegramRuntime) -> Result<(), AppError> {
    let bot = runtime.notifier.bot();
    let state = Arc::new(runtime);
    let handler = dptree::entry()
        .branch(Update::filter_message().endpoint(handle_message))
        .branch(Update::filter_callback_query().endpoint(handle_callback_query));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state])
        .build()
        .dispatch()
        .await;

    Ok(())
}

async fn handle_message(
    bot: Bot,
    message: Message,
    state: Arc<TelegramRuntime>,
) -> Result<(), teloxide::RequestError> {
    let Some(incoming_message) = to_incoming_message(&message) else {
        return Ok(());
    };
    let actor = match register_actor(&bot, &state, &incoming_message).await? {
        Some(actor) => actor,
        None => return Ok(()),
    };
    if !check_rate_limit(&bot, &state, &actor, message.chat.id).await? {
        return Ok(());
    }

    if let Some(command_text) = incoming_message.text_payload() {
        if let Some(command) = parse_command(command_text) {
            return handle_command(&bot, &state, incoming_message, actor, command).await;
        }
    }

    if let Some(session) = state.creation_sessions.get(incoming_message.chat_id).await {
        return handle_creation_session_message(&bot, &state, incoming_message, actor, session)
            .await;
    }
    if let Some(session) = state.task_interactions.get(incoming_message.chat_id).await {
        return handle_task_interaction_message(&bot, &state, incoming_message, actor, session)
            .await;
    }

    if matches!(
        &incoming_message.content,
        crate::domain::message::MessageContent::Voice { .. }
    ) {
        return VoiceCreateCoordinator::new(&bot, &state)
            .start(&actor, incoming_message)
            .await;
    }

    create_task_and_present(
        &bot,
        &state,
        ChatId(message.chat.id.0),
        incoming_message,
        SessionCompletion::Clear,
    )
    .await
}

async fn handle_callback_query(
    bot: Bot,
    callback_query: CallbackQuery,
    state: Arc<TelegramRuntime>,
) -> Result<(), teloxide::RequestError> {
    let Some(callback_data) = callback_query.data.as_deref() else {
        return Ok(());
    };
    let Some(callback) = crate::presentation::telegram::callbacks::parse_callback(callback_data)
    else {
        return Ok(());
    };
    let Some(incoming_message) = callback_to_incoming_message(&callback_query) else {
        return Ok(());
    };

    state
        .active_screens
        .hydrate_if_missing(incoming_message.chat_id, incoming_message.message_id)
        .await;

    let actor = match register_actor(&bot, &state, &incoming_message).await? {
        Some(actor) => actor,
        None => return Ok(()),
    };

    let is_stale_callback = state
        .active_screens
        .is_stale(incoming_message.chat_id, incoming_message.message_id)
        .await;
    let is_descriptor_mismatch = state
        .active_screens
        .get(incoming_message.chat_id)
        .await
        .is_some_and(|screen| !callback_matches_screen(&callback, &screen.descriptor));
    if is_stale_callback && callback.is_mutating() {
        answer_callback(
            &bot,
            &callback_query.id.to_string(),
            STALE_MUTATION_CALLBACK_TEXT,
        )
        .await?;
        return Ok(());
    }
    if !is_stale_callback && is_descriptor_mismatch && callback.is_mutating() {
        answer_callback(
            &bot,
            &callback_query.id.to_string(),
            STALE_MUTATION_CALLBACK_TEXT,
        )
        .await?;
        return Ok(());
    }

    let callback_answer = if is_stale_callback || is_descriptor_mismatch {
        STALE_NAVIGATION_CALLBACK_TEXT
    } else {
        CALLBACK_OK_TEXT
    };
    answer_callback(&bot, &callback_query.id.to_string(), callback_answer).await?;

    if matches!(
        callback,
        TelegramCallback::RegistrationPickEmployee { .. }
            | TelegramCallback::RegistrationContinueUnlinked
    ) {
        return handle_registration_link_callback(&bot, &state, incoming_message, callback).await;
    }

    handle_callback_action(&bot, &state, incoming_message, actor, callback).await
}

fn callback_matches_screen(callback: &TelegramCallback, descriptor: &ScreenDescriptor) -> bool {
    match callback {
        TelegramCallback::MenuHome
        | TelegramCallback::MenuHelp
        | TelegramCallback::MenuSettings
        | TelegramCallback::MenuStats
        | TelegramCallback::MenuTeamStats
        | TelegramCallback::MenuCreate
        | TelegramCallback::MenuSyncEmployees
        | TelegramCallback::ListTasks { .. }
        | TelegramCallback::OpenTask { .. } => true,
        TelegramCallback::UpdateTaskStatus { task_uid, .. }
        | TelegramCallback::ConfirmTaskCancel { task_uid, .. }
        | TelegramCallback::ExecuteTaskCancel { task_uid, .. }
        | TelegramCallback::StartTaskCommentInput { task_uid, .. }
        | TelegramCallback::StartTaskBlockerInput { task_uid, .. }
        | TelegramCallback::StartTaskReassignInput { task_uid, .. }
        | TelegramCallback::ShowDeliveryHelp { task_uid, .. } => {
            matches!(
                descriptor,
                ScreenDescriptor::TaskDetail {
                    task_uid: active_task_uid,
                    ..
                } if active_task_uid == task_uid
            ) || matches!(
                descriptor,
                ScreenDescriptor::CancelConfirmation {
                    task_uid: active_task_uid,
                    ..
                } if active_task_uid == task_uid
            )
        }
        TelegramCallback::StartQuickCreate | TelegramCallback::StartGuidedCreate => matches!(
            descriptor,
            ScreenDescriptor::CreateMenu
                | ScreenDescriptor::MainMenu
                | ScreenDescriptor::TaskCreationResult { .. }
        ),
        TelegramCallback::VoiceCreateConfirm
        | TelegramCallback::VoiceCreateEdit
        | TelegramCallback::VoiceCreateBack
        | TelegramCallback::VoiceCreateCancel => {
            matches!(descriptor, ScreenDescriptor::VoiceCreate(_))
        }
        TelegramCallback::RegistrationPickEmployee { .. }
        | TelegramCallback::RegistrationContinueUnlinked => {
            matches!(descriptor, ScreenDescriptor::RegistrationLinking)
        }
        TelegramCallback::ClarificationPickEmployee { .. }
        | TelegramCallback::ClarificationCreateUnassigned => matches!(
            descriptor,
            ScreenDescriptor::TaskCreationResult { .. }
                | ScreenDescriptor::TaskInteractionPrompt { .. }
                | ScreenDescriptor::VoiceCreate(_)
                | ScreenDescriptor::GuidedStep(_)
        ),
        TelegramCallback::DraftSkipAssignee | TelegramCallback::DraftEdit { .. } => {
            matches!(descriptor, ScreenDescriptor::GuidedStep(_))
        }
        TelegramCallback::DraftSkipDeadline | TelegramCallback::DraftSubmit => {
            matches!(descriptor, ScreenDescriptor::GuidedStep(_))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::callback_matches_screen;
    use crate::presentation::telegram::active_screens::ScreenDescriptor;
    use crate::presentation::telegram::callbacks::{
        TaskCardMode, TaskListOrigin, TelegramCallback,
    };
    use crate::presentation::telegram::drafts::VoiceTaskStep;
    use uuid::Uuid;

    #[test]
    fn given_task_mutation_callback_when_screen_points_to_other_task_then_detects_mismatch() {
        let callback_task_uid = Uuid::now_v7();
        let screen_task_uid = Uuid::now_v7();
        let callback = TelegramCallback::ExecuteTaskCancel {
            task_uid: callback_task_uid,
            origin: TaskListOrigin::Created,
        };
        let descriptor = ScreenDescriptor::TaskDetail {
            task_uid: screen_task_uid,
            mode: TaskCardMode::Compact,
            origin: TaskListOrigin::Created,
        };

        assert!(!callback_matches_screen(&callback, &descriptor));
    }

    #[test]
    fn given_voice_callback_when_voice_screen_active_then_it_is_accepted() {
        let callback = TelegramCallback::VoiceCreateConfirm;
        let descriptor = ScreenDescriptor::VoiceCreate(VoiceTaskStep::Confirm);

        assert!(callback_matches_screen(&callback, &descriptor));
    }
}
