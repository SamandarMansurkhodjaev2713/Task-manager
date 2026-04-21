use teloxide::prelude::Requester;
use teloxide::types::ChatId;
use teloxide::Bot;
use uuid::Uuid;

use crate::application::context::{PrincipalContext, TelegramChatContext};
use crate::application::use_cases::collect_stats::StatsScope;
use crate::application::use_cases::onboarding::OnboardingOutcome;
use crate::application::use_cases::register_user::RegistrationLinkPreview;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::{OnboardingState, User};
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::TelegramCallback;
use crate::presentation::telegram::commands::BotCommand;
use crate::presentation::telegram::ui;

use crate::shared::feature_flags::FeatureFlag;

use super::dispatcher_admin::{
    cancel_admin_pending, execute_admin_confirmation, prepare_admin_deactivate,
    prepare_admin_reactivate, prepare_admin_role_change, show_admin_audit, show_admin_features,
    show_admin_menu, show_admin_security_audit, show_admin_user_details, show_admin_users,
    toggle_admin_feature,
};
use super::dispatcher_onboarding::{render_outcome, OnboardingNextAction};

use super::dispatcher_assignee_clarifications::{
    choose_clarified_assignee, create_without_assignee_after_clarification,
};
use super::dispatcher_guided::{
    confirm_guided_assignee, create_task_and_present, edit_guided_field, skip_guided_assignee,
    skip_guided_deadline, start_guided_create, start_quick_create, submit_guided_draft,
    SessionCompletion,
};
use super::dispatcher_interactions::{
    start_task_blocker_input, start_task_comment_input, start_task_reassign_input,
};
use super::dispatcher_navigation::{
    show_create_menu, show_help, show_main_menu, show_main_menu_fresh, show_settings, show_stats,
    show_task_list, sync_employees,
};
use super::dispatcher_task_views::{
    confirm_task_cancel, execute_cancel_from_command, show_delivery_help, show_task_details,
    show_task_from_command, update_task_status,
};
use super::dispatcher_transport::{send_error, send_fresh_screen};
use super::dispatcher_voice::VoiceCreateCoordinator;
use super::TelegramRuntime;
use super::RATE_LIMIT_MESSAGE;

pub(crate) use super::dispatcher_task_views::{show_task_details_with_notice, TaskScreenContext};

/// Outcome of `register_actor`.  Drives the dispatcher's choice between
/// continuing with business logic and *stopping right here* so that the same
/// inbound Telegram update cannot produce two competing UX effects.
///
/// The previous `Option<User>` signature was ambiguous: both "onboarding
/// rendered a reply" and "registration bailed out" mapped to `None`, which
/// is exactly what let the onboarding-completion double-reply slip through
/// — the caller had no way to tell that a *screen* had already been sent.
pub(crate) enum RegistrationResult {
    /// The actor is ready; the dispatcher may continue to its business
    /// handlers (command, callback, or task-creation path).
    ///
    /// Boxed to keep the enum size uniform — `User` is ~232 bytes while the
    /// unit variants carry nothing.  This satisfies `clippy::large_enum_variant`.
    Ready(Box<User>),
    /// The onboarding gate has already rendered the only UX effect for this
    /// update.  The caller MUST return immediately and MUST NOT run any
    /// further handler that could emit another message.
    ConsumedByOnboarding,
    /// Registration failed or the user is pending a clarification; any
    /// user-facing error has already been sent.  Dispatch ends here.
    Aborted,
}

pub(crate) async fn register_actor(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: &IncomingMessage,
) -> Result<RegistrationResult, teloxide::RequestError> {
    // Onboarding v2 short-circuit.  If the user has not completed onboarding
    // (or does not yet exist), we route *all* traffic through the onboarding
    // FSM until `onboarding_state = completed`.  Legacy registration remains
    // intact for already-onboarded users and for task-recovery.
    match run_onboarding_gate(bot, state, incoming_message).await? {
        // On the exact update that finished onboarding we MUST NOT let the
        // dispatcher fall through: the user's message payload (e.g. their
        // last_name) would otherwise be reinterpreted as a task input.  The
        // legacy code returned `Ok(Some(actor))` here which is exactly what
        // produced the "Welcome" + "Incorrect request" double reply.
        OnboardingGateResult::JustCompleted | OnboardingGateResult::InProgress => {
            return Ok(RegistrationResult::ConsumedByOnboarding)
        }
        OnboardingGateResult::NotApplicable => {}
    }

    match state
        .register_user_use_case
        .preview_linking(incoming_message)
        .await
    {
        Ok(RegistrationLinkPreview::Ready(decision)) => match state
            .register_user_use_case
            .execute_with_link_decision(incoming_message, decision)
            .await
        {
            Ok(actor) => Ok(RegistrationResult::Ready(Box::new(actor))),
            Err(error) => {
                send_error(bot, state, incoming_message.chat_id, error).await?;
                Ok(RegistrationResult::Aborted)
            }
        },
        Ok(RegistrationLinkPreview::ClarificationRequired(clarification)) => {
            state
                .registration_links
                .set(
                    incoming_message.chat_id,
                    clarification
                        .candidates
                        .iter()
                        .filter_map(|candidate| candidate.employee_id)
                        .collect(),
                    clarification.allow_continue_unlinked,
                )
                .await;
            send_fresh_screen(
                bot,
                state,
                ChatId(incoming_message.chat_id),
                ScreenDescriptor::RegistrationLinking,
                &ui::registration_link_text(&clarification.message, &clarification.candidates),
                ui::registration_link_keyboard(
                    &clarification.candidates,
                    clarification.allow_continue_unlinked,
                ),
            )
            .await?;
            Ok(RegistrationResult::Aborted)
        }
        Err(error) => {
            send_error(bot, state, incoming_message.chat_id, error).await?;
            Ok(RegistrationResult::Aborted)
        }
    }
}

enum OnboardingGateResult {
    /// Onboarding was completed **by this very update** (e.g. user entered
    /// their last name).  We've already rendered the welcome screen; the
    /// dispatcher must stop to avoid a double reply.
    JustCompleted,
    /// Onboarding is still in progress; reply was already sent.
    InProgress,
    /// The user is fully onboarded and this update is not onboarding-related.
    NotApplicable,
}

async fn run_onboarding_gate(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: &IncomingMessage,
) -> Result<OnboardingGateResult, teloxide::RequestError> {
    let existing: Option<User> = state
        .onboarding_use_case
        .probe_onboarding_state(incoming_message.sender_id)
        .await
        .unwrap_or_default();

    // Already fully onboarded with first/last name — legacy path.
    if let Some(user) = existing.as_ref() {
        if user.onboarding_state == OnboardingState::Completed
            && user.first_name.as_deref().is_some_and(|v| !v.is_empty())
            && user.last_name.as_deref().is_some_and(|v| !v.is_empty())
        {
            return Ok(OnboardingGateResult::NotApplicable);
        }
    }

    let ctx = PrincipalContext::anonymous(
        TelegramChatContext {
            chat_id: incoming_message.chat_id,
            telegram_user_id: incoming_message.sender_id,
        },
        Uuid::new_v4(),
        incoming_message.timestamp,
    );

    let is_start_command = incoming_message
        .text_payload()
        .is_some_and(|payload| payload.trim_start().starts_with("/start"));

    let outcome_result = if existing.is_none() || is_start_command {
        state
            .onboarding_use_case
            .handle_start(&ctx, incoming_message)
            .await
    } else if let Some(text) = incoming_message.text_payload().map(str::to_owned) {
        state
            .onboarding_use_case
            .handle_text(
                &ctx,
                incoming_message,
                crate::application::use_cases::onboarding::OnboardingTextInput { text },
            )
            .await
    } else {
        // Voice / sticker / photo while in onboarding: reprompt.
        return {
            let _ = bot
                .send_message(
                    ChatId(incoming_message.chat_id),
                    ui::onboarding_welcome_text(),
                )
                .await?;
            Ok(OnboardingGateResult::InProgress)
        };
    };

    match outcome_result {
        Ok(OnboardingOutcome::Completed { user }) => {
            let action = render_outcome(
                bot,
                state,
                ChatId(incoming_message.chat_id),
                OnboardingOutcome::Completed { user: user.clone() },
            )
            .await?;
            let _ = action;
            // CRITICAL: this branch fires on the exact update that finished
            // onboarding.  Returning `Completed(actor)` here used to let the
            // dispatcher run `create_task_and_present` on the same message
            // (last_name, first_name, etc.), which produced a second reply
            // ("Некорректный запрос. Проверьте данные и попробуйте снова.")
            // after the welcome screen.  We now return `JustCompleted` so the
            // caller stops immediately.  The next update will load the user
            // normally and fall through `NotApplicable` into business logic.
            let _ = user;
            Ok(OnboardingGateResult::JustCompleted)
        }
        Ok(outcome) => {
            let action =
                render_outcome(bot, state, ChatId(incoming_message.chat_id), outcome).await?;
            match action {
                OnboardingNextAction::Completed => {
                    // Same argument as above: the render just finished
                    // onboarding on this update; do not let the dispatcher
                    // continue.
                    Ok(OnboardingGateResult::JustCompleted)
                }
                OnboardingNextAction::AwaitText | OnboardingNextAction::AwaitCallback => {
                    Ok(OnboardingGateResult::InProgress)
                }
            }
        }
        Err(error) => {
            send_error(bot, state, incoming_message.chat_id, error).await?;
            Ok(OnboardingGateResult::InProgress)
        }
    }
}

pub(crate) async fn check_rate_limit(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<bool, teloxide::RequestError> {
    // Telegram user IDs are always non-negative; bit-cast is safe and avoids
    // the unwrap_or_default() footgun that would bucket all negative IDs into key=0.
    let actor_key = actor.telegram_id as u64;
    if state.rate_limiter.check(actor_key) {
        return Ok(true);
    }

    // Record the rate-limit event in the security audit log so ops can
    // identify storms without needing to grep structured logs.
    let entry = crate::domain::audit::SecurityAuditEntry {
        id: None,
        actor_user_id: actor.id,
        telegram_id: Some(actor.telegram_id),
        event_code: crate::domain::audit::AuditActionCode::RateLimitExceeded,
        metadata: serde_json::json!({
            "chat_id": chat_id.0,
        }),
        created_at: chrono::Utc::now(),
    };
    if let Err(err) = state.security_audit.append(&entry).await {
        tracing::warn!(
            telegram_id = actor.telegram_id,
            error = %err,
            "failed to record rate-limit security event"
        );
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
        BotCommand::Start | BotCommand::Menu => {
            show_main_menu_fresh(bot, state, &actor, chat_id).await
        }
        BotCommand::Help => show_help(bot, state, chat_id, &actor).await,
        BotCommand::NewTask { payload } => {
            handle_new_task_command(bot, state, incoming_message, chat_id, payload).await
        }
        BotCommand::MyTasks { cursor } => {
            show_task_list(
                bot,
                state,
                &actor,
                chat_id,
                crate::presentation::telegram::callbacks::TaskListOrigin::Assigned,
                cursor,
            )
            .await
        }
        BotCommand::CreatedTasks { cursor } => {
            show_task_list(
                bot,
                state,
                &actor,
                chat_id,
                crate::presentation::telegram::callbacks::TaskListOrigin::Created,
                cursor,
            )
            .await
        }
        BotCommand::TeamTasks { cursor } => {
            show_task_list(
                bot,
                state,
                &actor,
                chat_id,
                crate::presentation::telegram::callbacks::TaskListOrigin::Team,
                cursor,
            )
            .await
        }
        BotCommand::TeamStats => {
            if !state
                .feature_flags
                .read()
                .await
                .is_enabled(FeatureFlag::TeamAnalytics)
            {
                return send_fresh_screen(
                    bot,
                    state,
                    chat_id,
                    ScreenDescriptor::MainMenu,
                    "📊 Командная аналитика пока не включена.\nОбратитесь к администратору.",
                    ui::main_menu_keyboard(&actor),
                )
                .await;
            }
            show_stats(bot, state, &actor, chat_id, StatsScope::Team).await
        }
        BotCommand::Status { task_uid } => {
            show_task_from_command(bot, state, &actor, chat_id, &task_uid).await
        }
        BotCommand::CancelTask { task_uid } => {
            execute_cancel_from_command(bot, state, &actor, chat_id, &task_uid).await
        }
        BotCommand::Stats => show_stats(bot, state, &actor, chat_id, StatsScope::Personal).await,
        BotCommand::Settings => show_settings(bot, state, chat_id, &actor).await,
        BotCommand::AdminSyncEmployees => sync_employees(bot, state, chat_id, &actor).await,
        BotCommand::Admin => show_admin_menu(bot, state, &actor, chat_id).await,
        BotCommand::Find { query } => run_find_command(bot, state, &actor, chat_id, query).await,
    }
}

/// Phase-10 skeleton handler for `/find <query>`.  Invokes `SearchTasksUseCase`
/// when a query is present and renders a short, text-only result list.  When
/// no query is supplied we return a usage hint rather than an error toast so
/// that discoverability stays high.
async fn run_find_command(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    query: Option<String>,
) -> Result<(), teloxide::RequestError> {
    use crate::application::use_cases::search_tasks::SearchQuery;

    let Some(raw) = query else {
        let hint = "🔍 Введите поиск: /find <текст>\n\nИскать можно по названию, описанию или коду задачи.";
        return send_fresh_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::Help,
            hint,
            ui::main_menu_keyboard(actor),
        )
        .await;
    };

    let parsed = match SearchQuery::parse(&raw) {
        Ok(q) => q,
        Err(error) => return send_error(bot, state, chat_id.0, error).await,
    };

    match state.search_tasks_use_case.execute(actor, &parsed).await {
        Ok(items) => {
            let body = if items.is_empty() {
                format!("🔍 По запросу «{}» ничего не найдено.", parsed.canonical())
            } else {
                let mut out = format!(
                    "🔍 Найдено: {} (по запросу «{}»)\n",
                    items.len(),
                    parsed.canonical()
                );
                for item in items.iter().take(10) {
                    out.push_str(&format!("\n• {} — {}", item.public_code, item.title));
                }
                if items.len() > 10 {
                    out.push_str(&format!("\n…и ещё {} задач", items.len() - 10));
                }
                out
            };
            send_fresh_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::Help,
                &body,
                ui::main_menu_keyboard(actor),
            )
            .await
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
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
        TelegramCallback::MenuHelp => show_help(bot, state, chat_id, &actor).await,
        TelegramCallback::MenuSettings => show_settings(bot, state, chat_id, &actor).await,
        TelegramCallback::MenuStats => {
            show_stats(bot, state, &actor, chat_id, StatsScope::Personal).await
        }
        TelegramCallback::MenuTeamStats => {
            if !state
                .feature_flags
                .read()
                .await
                .is_enabled(FeatureFlag::TeamAnalytics)
            {
                return show_main_menu(bot, state, &actor, chat_id).await;
            }
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
                crate::domain::task::TaskStatus::Cancelled,
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
        TelegramCallback::ShowDeliveryHelp { task_uid, origin } => {
            show_delivery_help(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::StartQuickCreate => start_quick_create(bot, state, chat_id).await,
        TelegramCallback::StartGuidedCreate => start_guided_create(bot, state, chat_id).await,
        TelegramCallback::VoiceCreateConfirm
        | TelegramCallback::VoiceCreateEdit
        | TelegramCallback::VoiceCreateBack
        | TelegramCallback::VoiceCreateCancel => {
            if !state
                .feature_flags
                .read()
                .await
                .is_enabled(FeatureFlag::VoiceV2)
            {
                return show_main_menu(bot, state, &actor, chat_id).await;
            }
            match callback {
                TelegramCallback::VoiceCreateConfirm => {
                    VoiceCreateCoordinator::new(bot, state)
                        .submit(&actor, chat_id)
                        .await
                }
                TelegramCallback::VoiceCreateEdit => {
                    VoiceCreateCoordinator::new(bot, state)
                        .start_edit(chat_id)
                        .await
                }
                TelegramCallback::VoiceCreateBack => {
                    VoiceCreateCoordinator::new(bot, state)
                        .return_to_confirmation(&actor, chat_id)
                        .await
                }
                _ => {
                    // VoiceCreateCancel
                    VoiceCreateCoordinator::new(bot, state)
                        .cancel(chat_id)
                        .await
                }
            }
        }
        TelegramCallback::RegistrationPickEmployee { .. }
        | TelegramCallback::RegistrationContinueUnlinked => Ok(()),
        TelegramCallback::ClarificationPickEmployee { employee_id } => {
            choose_clarified_assignee(bot, state, &actor, chat_id, employee_id).await
        }
        TelegramCallback::ClarificationCreateUnassigned => {
            create_without_assignee_after_clarification(bot, state, chat_id).await
        }
        TelegramCallback::DraftSkipAssignee => skip_guided_assignee(bot, state, chat_id).await,
        TelegramCallback::DraftSkipDeadline => skip_guided_deadline(bot, state, chat_id).await,
        TelegramCallback::DraftSubmit => submit_guided_draft(bot, state, &actor, chat_id).await,
        TelegramCallback::DraftEdit { field } => {
            edit_guided_field(bot, state, chat_id, field).await
        }
        TelegramCallback::GuidedAssigneeConfirm { employee_id } => {
            confirm_guided_assignee(bot, state, chat_id, employee_id).await
        }
        TelegramCallback::AdminMenu => show_admin_menu(bot, state, &actor, chat_id).await,
        TelegramCallback::AdminUsers => show_admin_users(bot, state, &actor, chat_id).await,
        TelegramCallback::AdminUserDetails { user_id } => {
            show_admin_user_details(bot, state, &actor, chat_id, user_id).await
        }
        TelegramCallback::AdminUserPrepareRoleChange { user_id, next_role } => {
            prepare_admin_role_change(bot, state, &actor, chat_id, user_id, next_role).await
        }
        TelegramCallback::AdminUserPrepareDeactivate { user_id } => {
            prepare_admin_deactivate(bot, state, &actor, chat_id, user_id).await
        }
        TelegramCallback::AdminUserPrepareReactivate { user_id } => {
            prepare_admin_reactivate(bot, state, &actor, chat_id, user_id).await
        }
        TelegramCallback::AdminConfirmNonce { nonce } => {
            execute_admin_confirmation(bot, state, &actor, chat_id, nonce).await
        }
        TelegramCallback::AdminCancelPending => {
            cancel_admin_pending(bot, state, &actor, chat_id).await
        }
        TelegramCallback::AdminAudit => show_admin_audit(bot, state, &actor, chat_id).await,
        TelegramCallback::AdminSecurityAudit => {
            show_admin_security_audit(bot, state, &actor, chat_id).await
        }
        TelegramCallback::AdminFeatures => show_admin_features(bot, state, &actor, chat_id).await,
        TelegramCallback::AdminToggleFeature { flag_key } => {
            toggle_admin_feature(bot, state, &actor, chat_id, flag_key).await
        }
    }
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
