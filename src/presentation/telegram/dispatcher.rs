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
use crate::presentation::telegram::commands::parse_command;
use crate::presentation::telegram::drafts::CreationSessionStore;
use crate::presentation::telegram::interactions::TaskInteractionSessionStore;
use crate::presentation::telegram::rate_limiter::TelegramRateLimiter;

use self::dispatcher_guided::{
    create_task_and_present, handle_creation_session_message, SessionCompletion,
};
use self::dispatcher_handlers::{
    check_rate_limit, handle_callback_action, handle_command, register_actor,
};
use self::dispatcher_interactions::handle_task_interaction_message;
use self::dispatcher_transport::{
    answer_callback, callback_to_incoming_message, to_incoming_message,
};

#[path = "dispatcher_guided.rs"]
mod dispatcher_guided;
#[path = "dispatcher_handlers.rs"]
mod dispatcher_handlers;
#[path = "dispatcher_interactions.rs"]
mod dispatcher_interactions;
#[path = "dispatcher_transport.rs"]
mod dispatcher_transport;

pub(crate) const CALLBACK_OK_TEXT: &str = "Готово";
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
    let actor = match register_actor(&bot, &state, &incoming_message).await? {
        Some(actor) => actor,
        None => return Ok(()),
    };

    answer_callback(&bot, &callback_query.id.to_string(), CALLBACK_OK_TEXT).await?;
    handle_callback_action(&bot, &state, incoming_message, actor, callback).await
}
