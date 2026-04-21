use std::sync::Arc;

use teloxide::dispatching::UpdateFilterExt;
use teloxide::dptree;
use teloxide::prelude::{CallbackQuery, Dispatcher, Message, Update};
use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::ports::repositories::SecurityAuditLogRepository;
use crate::application::use_cases::add_task_comment::AddTaskCommentUseCase;
use crate::application::use_cases::admin::AdminUseCase;
use crate::application::use_cases::collect_stats::CollectStatsUseCase;
use crate::application::use_cases::create_task_from_message::CreateTaskFromMessageUseCase;
use crate::application::use_cases::get_task_status::GetTaskStatusUseCase;
use crate::application::use_cases::list_tasks::ListTasksUseCase;
use crate::application::use_cases::onboarding::OnboardingUseCase;
use crate::application::use_cases::reassign_task::ReassignTaskUseCase;
use crate::application::use_cases::register_user::RegisterUserUseCase;
use crate::application::use_cases::report_task_blocker::ReportTaskBlockerUseCase;
use crate::application::use_cases::search_tasks::SearchTasksUseCase;
use crate::application::use_cases::sync_employees::SyncEmployeesUseCase;
use crate::application::use_cases::update_task_status::UpdateTaskStatusUseCase;
use crate::domain::errors::AppError;
use crate::infrastructure::telegram::bot_gateway::TeloxideNotifier;
use crate::presentation::telegram::active_screens::ActiveScreenStore;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::admin_nonce_store::AdminNonceStore;
use crate::presentation::telegram::assignee_selections::PendingAssigneeSelectionStore;
use crate::presentation::telegram::callbacks::TelegramCallback;
use crate::presentation::telegram::commands::parse_command;
use crate::presentation::telegram::drafts::CreationSessionStore;
use crate::presentation::telegram::gateway::{ChatSerializer, DedupKey, UpdateDedup, UxBarrier};
use crate::presentation::telegram::interactions::TaskInteractionSessionStore;
use crate::presentation::telegram::rate_limiter::TelegramRateLimiter;
use crate::presentation::telegram::registration_links::PendingRegistrationLinkStore;
use crate::shared::feature_flags::SharedFeatureFlagRegistry;

use std::collections::HashMap;
use tokio::sync::Mutex as AsyncMutex;

use self::dispatcher_guided::{
    create_task_and_present, handle_creation_session_message, SessionCompletion,
};
use self::dispatcher_handlers::{
    check_rate_limit, handle_callback_action, handle_command, register_actor, RegistrationResult,
};
use self::dispatcher_interactions::handle_task_interaction_message;
use self::dispatcher_registration::handle_registration_link_callback;
use self::dispatcher_transport::{
    answer_callback, callback_to_incoming_message, to_incoming_message,
};
use self::dispatcher_voice::VoiceCreateCoordinator;

#[path = "dispatcher_admin.rs"]
pub(crate) mod dispatcher_admin;
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
#[path = "dispatcher_onboarding.rs"]
mod dispatcher_onboarding;
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
    pub admin_nonce_store: AdminNonceStore,
    pub register_user_use_case: Arc<RegisterUserUseCase>,
    pub onboarding_use_case: Arc<OnboardingUseCase>,
    pub create_task_use_case: Arc<CreateTaskFromMessageUseCase>,
    pub list_tasks_use_case: Arc<ListTasksUseCase>,
    pub get_task_status_use_case: Arc<GetTaskStatusUseCase>,
    pub update_task_status_use_case: Arc<UpdateTaskStatusUseCase>,
    pub add_task_comment_use_case: Arc<AddTaskCommentUseCase>,
    pub report_task_blocker_use_case: Arc<ReportTaskBlockerUseCase>,
    pub reassign_task_use_case: Arc<ReassignTaskUseCase>,
    pub collect_stats_use_case: Arc<CollectStatsUseCase>,
    pub sync_employees_use_case: Arc<SyncEmployeesUseCase>,
    pub admin_use_case: Arc<AdminUseCase>,
    pub search_tasks_use_case: Arc<SearchTasksUseCase>,
    /// Live feature flag state shared with the admin use case.  Handlers that
    /// need to read or display flag state use `feature_flags.read().await`.
    pub feature_flags: SharedFeatureFlagRegistry,
    /// Append-only log of security-sensitive events (rate-limit storms,
    /// forbidden-action attempts, callback authorship mismatches).  Failures
    /// are logged but never propagated — audit misses must not break UX.
    pub security_audit: Arc<dyn SecurityAuditLogRepository>,

    // Per-chat serialization + UX barrier for the "single outbound effect per
    // update" invariant.  Populated by [`run_telegram_dispatcher`]; every
    // message/callback handler pushes a fresh [`UxBarrier`] into
    // `current_barriers` for the duration of the critical section and pops it
    // on exit.  Transport helpers consult `current_barrier_for` before sending.
    pub chat_serializer: ChatSerializer,
    pub update_dedup: UpdateDedup,
    pub current_barriers: Arc<AsyncMutex<HashMap<i64, UxBarrier>>>,
}

impl TelegramRuntime {
    /// Returns the [`UxBarrier`] that is currently active for `chat_id`, if
    /// the update is being handled inside the per-chat critical section.
    /// Transport helpers fall back to `None` outside of an update context
    /// (background jobs, tests), in which case they render unconditionally.
    pub async fn current_barrier_for(&self, chat_id: i64) -> Option<UxBarrier> {
        self.current_barriers.lock().await.get(&chat_id).cloned()
    }

    async fn install_barrier(&self, chat_id: i64, barrier: UxBarrier) {
        self.current_barriers.lock().await.insert(chat_id, barrier);
    }

    async fn uninstall_barrier(&self, chat_id: i64) {
        self.current_barriers.lock().await.remove(&chat_id);
    }
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

    // Acquire per-chat serialization *before* any business logic so that two
    // concurrent updates from the same chat never interleave.  Different
    // chats keep running in parallel because the lock map is keyed by chat_id.
    let chat_id = incoming_message.chat_id;
    let guard = state.chat_serializer.acquire(chat_id).await;

    // Dedup retried webhooks: Telegram may redeliver an update if we fail to
    // acknowledge it in time.  (chat_id, message_id) is the canonical
    // fingerprint for messages.
    let dedup_key = DedupKey {
        chat_id,
        token: i64::from(incoming_message.message_id),
    };
    if !state.update_dedup.observe(dedup_key).await {
        tracing::info!(
            chat_id,
            message_id = incoming_message.message_id,
            "duplicate message update dropped"
        );
        return Ok(());
    }

    state.install_barrier(chat_id, guard.barrier()).await;
    let result = dispatch_message_inner(&bot, state.as_ref(), message, incoming_message).await;
    state.uninstall_barrier(chat_id).await;
    drop(guard);
    result
}

async fn dispatch_message_inner(
    bot: &Bot,
    state: &TelegramRuntime,
    message: Message,
    incoming_message: crate::domain::message::IncomingMessage,
) -> Result<(), teloxide::RequestError> {
    let registration = register_actor(bot, state, &incoming_message).await?;
    let actor = match registration {
        // Onboarding has just rendered its screen for this update — we MUST
        // NOT run any further business handler, otherwise the same raw text
        // (e.g. the last_name input) would be interpreted as a new task.
        RegistrationResult::ConsumedByOnboarding => return Ok(()),
        RegistrationResult::Aborted => return Ok(()),
        RegistrationResult::Ready(actor) => *actor,
    };
    if !check_rate_limit(bot, state, &actor, message.chat.id).await? {
        return Ok(());
    }

    if let Some(command_text) = incoming_message.text_payload() {
        if let Some(command) = parse_command(command_text) {
            return handle_command(bot, state, incoming_message, actor, command).await;
        }
    }

    if let Some(session) = state.creation_sessions.get(incoming_message.chat_id).await {
        return handle_creation_session_message(bot, state, incoming_message, actor, session).await;
    }
    if let Some(session) = state.task_interactions.get(incoming_message.chat_id).await {
        return handle_task_interaction_message(bot, state, incoming_message, actor, session).await;
    }

    if matches!(
        &incoming_message.content,
        crate::domain::message::MessageContent::Voice { .. }
    ) {
        return VoiceCreateCoordinator::new(bot, state)
            .start(&actor, incoming_message)
            .await;
    }

    create_task_and_present(
        bot,
        state,
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

    let chat_id = incoming_message.chat_id;
    let guard = state.chat_serializer.acquire(chat_id).await;

    // Callback IDs are unique per callback press; hashing them into i64 gives
    // a stable dedup token for webhook retries.  We intentionally do not
    // dedupe on (chat_id, message_id) here because a Telegram "message" can
    // legitimately receive multiple independent button presses.
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    callback_query.id.hash(&mut hasher);
    let dedup_token = hasher.finish() as i64;
    let dedup_key = DedupKey {
        chat_id,
        token: dedup_token,
    };
    if !state.update_dedup.observe(dedup_key).await {
        tracing::info!(chat_id, "duplicate callback update dropped");
        return Ok(());
    }

    state.install_barrier(chat_id, guard.barrier()).await;
    let result = dispatch_callback_inner(
        &bot,
        state.as_ref(),
        callback_query,
        callback,
        incoming_message,
    )
    .await;
    state.uninstall_barrier(chat_id).await;
    drop(guard);
    result
}

async fn dispatch_callback_inner(
    bot: &Bot,
    state: &TelegramRuntime,
    callback_query: CallbackQuery,
    callback: TelegramCallback,
    incoming_message: crate::domain::message::IncomingMessage,
) -> Result<(), teloxide::RequestError> {
    state
        .active_screens
        .hydrate_if_missing(incoming_message.chat_id, incoming_message.message_id)
        .await;

    let registration = register_actor(bot, state, &incoming_message).await?;
    let actor = match registration {
        RegistrationResult::ConsumedByOnboarding | RegistrationResult::Aborted => return Ok(()),
        RegistrationResult::Ready(actor) => *actor,
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
            bot,
            &callback_query.id.to_string(),
            STALE_MUTATION_CALLBACK_TEXT,
        )
        .await?;
        return Ok(());
    }
    if !is_stale_callback && is_descriptor_mismatch && callback.is_mutating() {
        answer_callback(
            bot,
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
    answer_callback(bot, &callback_query.id.to_string(), callback_answer).await?;

    if matches!(
        callback,
        TelegramCallback::RegistrationPickEmployee { .. }
            | TelegramCallback::RegistrationContinueUnlinked
    ) {
        return handle_registration_link_callback(bot, state, incoming_message, callback).await;
    }

    handle_callback_action(bot, state, incoming_message, actor, callback).await
}

/// Thin adapter kept for backwards compatibility.  The real policy lives on
/// [`ScreenDescriptor::accepts`] (see
/// [`crate::presentation::telegram::active_screens::Stage`] for the stage
/// capability matrix).  Keeping the wrapper allows future presenters to
/// depend on a stable call site without coupling to the descriptor API.
fn callback_matches_screen(callback: &TelegramCallback, descriptor: &ScreenDescriptor) -> bool {
    descriptor.accepts(callback)
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
