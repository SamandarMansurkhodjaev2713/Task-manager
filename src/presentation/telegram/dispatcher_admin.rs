//! Telegram-side handlers for the admin panel (Phase 4).
//!
//! Every handler in this module:
//!
//! 1. Re-verifies the actor is an admin via `AdminUseCase` (defence in
//!    depth, even though the main dispatcher already applies the same
//!    gate).
//! 2. Translates domain errors to UI text — mapping `LAST_ADMIN_PROTECTED`,
//!    `FORBIDDEN_SELF_TARGET`, and friends to dedicated Russian messages.
//! 3. Leaves ALL DB writes to the use case layer — no SQL is issued from
//!    here directly.
//!
//! Destructive actions (role change, deactivate, reactivate) go through
//! the nonce store: the first callback *prepares* the action and emits a
//! confirmation screen, the second callback *consumes* the nonce.

use teloxide::types::ChatId;
use teloxide::Bot;

use crate::domain::errors::AppError;
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::admin_nonce_store::{NonceError, PendingAdminAction};
use crate::presentation::telegram::callbacks::AdminRoleOption;
use crate::presentation::telegram::ui;

use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

const RECENT_AUDIT_LIMIT: u32 = 20;
const RECENT_SECURITY_AUDIT_LIMIT: i64 = 30;
const LAST_ADMIN_CODE: &str = "LAST_ADMIN_PROTECTED";
const USER_NOT_FOUND_CODE: &str = "USER_NOT_FOUND";
const FORBIDDEN_ADMIN_ONLY: &str = "FORBIDDEN_ADMIN_ONLY";
const FORBIDDEN_ACCOUNT_DEACTIVATED: &str = "FORBIDDEN_ACCOUNT_DEACTIVATED";
const FORBIDDEN_SELF_TARGET: &str = "FORBIDDEN_SELF_TARGET";

pub(crate) async fn show_admin_menu(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    if let Err(error) = guard_admin(actor) {
        return render_admin_error(bot, state, actor, chat_id, error).await;
    }
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::AdminMenu,
        &ui::admin_menu_text(actor),
        ui::admin_menu_keyboard(),
    )
    .await
}

pub(crate) async fn show_admin_users(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    match state.admin_use_case.list_active_admins(actor).await {
        Ok(users) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminUsers,
                &ui::admin_users_text(&users, actor),
                ui::admin_users_keyboard(&users),
            )
            .await
        }
        Err(error) => render_admin_error(bot, state, actor, chat_id, error).await,
    }
}

pub(crate) async fn show_admin_user_details(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    user_id: i64,
) -> Result<(), teloxide::RequestError> {
    match state.admin_use_case.get_user(actor, user_id).await {
        Ok(target) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminUserDetails { user_id },
                &ui::admin_user_details_text(&target),
                ui::admin_user_details_keyboard(&target),
            )
            .await
        }
        Err(error) => render_admin_error(bot, state, actor, chat_id, error).await,
    }
}

pub(crate) async fn prepare_admin_role_change(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    user_id: i64,
    next_role: AdminRoleOption,
) -> Result<(), teloxide::RequestError> {
    let target = match state.admin_use_case.get_user(actor, user_id).await {
        Ok(target) => target,
        Err(error) => return render_admin_error(bot, state, actor, chat_id, error).await,
    };

    let display_name = user_display_name(&target);
    let action = PendingAdminAction::ChangeRole {
        target_user_id: user_id,
        target_telegram_id: target.telegram_id,
        display_name,
        next_role,
    };
    issue_confirmation(bot, state, actor, chat_id, action).await
}

pub(crate) async fn prepare_admin_deactivate(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    user_id: i64,
) -> Result<(), teloxide::RequestError> {
    let target = match state.admin_use_case.get_user(actor, user_id).await {
        Ok(target) => target,
        Err(error) => return render_admin_error(bot, state, actor, chat_id, error).await,
    };
    let action = PendingAdminAction::Deactivate {
        target_user_id: user_id,
        target_telegram_id: target.telegram_id,
        display_name: user_display_name(&target),
    };
    issue_confirmation(bot, state, actor, chat_id, action).await
}

pub(crate) async fn prepare_admin_reactivate(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    user_id: i64,
) -> Result<(), teloxide::RequestError> {
    let target = match state.admin_use_case.get_user(actor, user_id).await {
        Ok(target) => target,
        Err(error) => return render_admin_error(bot, state, actor, chat_id, error).await,
    };
    let action = PendingAdminAction::Reactivate {
        target_user_id: user_id,
        target_telegram_id: target.telegram_id,
        display_name: user_display_name(&target),
    };
    issue_confirmation(bot, state, actor, chat_id, action).await
}

pub(crate) async fn execute_admin_confirmation(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    nonce: String,
) -> Result<(), teloxide::RequestError> {
    let Some(actor_id) = actor.id else {
        return render_admin_error(
            bot,
            state,
            actor,
            chat_id,
            AppError::unauthenticated(
                "Actor must be registered",
                serde_json::json!({ "telegram_id": actor.telegram_id }),
            ),
        )
        .await;
    };
    let action = match state.admin_nonce_store.consume(actor_id, &nonce) {
        Ok(action) => action,
        Err(NonceError::NotFound) => {
            return send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminMenu,
                &ui::admin_nonce_expired_text(),
                ui::admin_menu_keyboard(),
            )
            .await;
        }
        Err(NonceError::WrongActor) => {
            return send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminMenu,
                &ui::admin_nonce_wrong_actor_text(),
                ui::admin_menu_keyboard(),
            )
            .await;
        }
    };

    let result = match action {
        PendingAdminAction::ChangeRole {
            target_user_id,
            next_role,
            ..
        } => state
            .admin_use_case
            .change_role(actor, target_user_id, next_role.to_user_role())
            .await
            .map(ExecutionOutcome::RoleChanged),
        PendingAdminAction::Deactivate { target_user_id, .. } => state
            .admin_use_case
            .deactivate_user(actor, target_user_id)
            .await
            .map(ExecutionOutcome::Deactivated),
        PendingAdminAction::Reactivate { target_user_id, .. } => state
            .admin_use_case
            .reactivate_user(actor, target_user_id)
            .await
            .map(ExecutionOutcome::Reactivated),
    };

    match result {
        Ok(ExecutionOutcome::RoleChanged(user)) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminUserDetails {
                    user_id: user.id.unwrap_or(0),
                },
                &ui::admin_role_changed_text(&user),
                ui::admin_user_details_keyboard(&user),
            )
            .await
        }
        Ok(ExecutionOutcome::Deactivated(user)) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminUserDetails {
                    user_id: user.id.unwrap_or(0),
                },
                &ui::admin_deactivated_text(&user),
                ui::admin_user_details_keyboard(&user),
            )
            .await
        }
        Ok(ExecutionOutcome::Reactivated(user)) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminUserDetails {
                    user_id: user.id.unwrap_or(0),
                },
                &ui::admin_reactivated_text(&user),
                ui::admin_user_details_keyboard(&user),
            )
            .await
        }
        Err(error) => render_admin_error(bot, state, actor, chat_id, error).await,
    }
}

pub(crate) async fn cancel_admin_pending(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::AdminMenu,
        &ui::admin_action_cancelled_text(),
        ui::admin_menu_keyboard(),
    )
    .await?;
    show_admin_menu(bot, state, actor, chat_id).await
}

pub(crate) async fn show_admin_audit(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    match state
        .admin_use_case
        .list_recent_audit(actor, RECENT_AUDIT_LIMIT)
        .await
    {
        Ok(entries) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminAudit,
                &ui::admin_audit_text(&entries),
                ui::admin_back_keyboard(),
            )
            .await
        }
        Err(error) => render_admin_error(bot, state, actor, chat_id, error).await,
    }
}

pub(crate) async fn show_admin_security_audit(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    if let Err(error) = guard_admin(actor) {
        return render_admin_error(bot, state, actor, chat_id, error).await;
    }
    match state
        .security_audit
        .list_recent(RECENT_SECURITY_AUDIT_LIMIT)
        .await
    {
        Ok(entries) => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminSecurityAudit,
                &ui::admin_security_audit_text(&entries),
                ui::admin_back_keyboard(),
            )
            .await
        }
        Err(error) => render_admin_error(bot, state, actor, chat_id, error).await,
    }
}

pub(crate) async fn show_admin_features(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    if let Err(error) = guard_admin(actor) {
        return render_admin_error(bot, state, actor, chat_id, error).await;
    }
    let all_flags = state.feature_flags.read().await.all_flags();
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::AdminFeatures,
        &ui::admin_features_text(&all_flags),
        ui::admin_features_keyboard(&all_flags),
    )
    .await
}

/// Toggles a feature flag and re-renders the features screen to reflect the
/// new state.
pub(crate) async fn toggle_admin_feature(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    flag_key: String,
) -> Result<(), teloxide::RequestError> {
    // Determine the current state so we can flip it.
    let current_enabled = {
        // Parse the flag first to avoid holding the lock across an await.
        use crate::shared::feature_flags::FeatureFlag;
        use std::str::FromStr;
        match FeatureFlag::from_str(&flag_key) {
            Ok(flag) => state.feature_flags.read().await.is_enabled(flag),
            Err(_) => {
                // Unknown flag — just re-render the screen unchanged.
                return show_admin_features(bot, state, actor, chat_id).await;
            }
        }
    };
    let new_enabled = !current_enabled;

    match state
        .admin_use_case
        .toggle_feature_flag(actor, &flag_key, new_enabled)
        .await
    {
        Ok(_) => show_admin_features(bot, state, actor, chat_id).await,
        Err(error) => render_admin_error(bot, state, actor, chat_id, error).await,
    }
}

async fn issue_confirmation(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    action: PendingAdminAction,
) -> Result<(), teloxide::RequestError> {
    let Some(actor_id) = actor.id else {
        return render_admin_error(
            bot,
            state,
            actor,
            chat_id,
            AppError::unauthenticated(
                "Actor must be registered",
                serde_json::json!({ "telegram_id": actor.telegram_id }),
            ),
        )
        .await;
    };

    // Re-verify self-target here so the UX error message takes precedence
    // over the nonce issuance.  This also means a leaked callback for
    // "change own role" is still refused even before consuming the nonce.
    if action.target_user_id() == actor_id {
        return render_admin_error(
            bot,
            state,
            actor,
            chat_id,
            AppError::forbidden(
                crate::application::policies::role_authorization::FORBIDDEN_SELF_TARGET,
                "An admin cannot apply this action to themselves",
                serde_json::json!({
                    "actor_user_id": actor_id,
                }),
            ),
        )
        .await;
    }

    let nonce = state.admin_nonce_store.issue(actor_id, action.clone());
    let body = ui::describe_pending_admin_action(&action);
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::AdminConfirm {
            nonce: nonce.clone(),
        },
        &ui::admin_confirm_text(&body),
        ui::admin_confirmation_keyboard(&nonce),
    )
    .await
}

fn guard_admin(actor: &User) -> Result<(), AppError> {
    crate::application::policies::role_authorization::RoleAuthorizationPolicy::ensure_can_access_admin_panel(actor)
}

async fn render_admin_error(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    error: AppError,
) -> Result<(), teloxide::RequestError> {
    match error.code() {
        FORBIDDEN_ADMIN_ONLY => {
            // Access-denied is deliberately sent as a plain screen instead of
            // the generic send_error banner so the user lands back in the
            // main menu (rather than on a broken admin screen).
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::MainMenu,
                &ui::admin_access_denied_text(),
                ui::main_menu_keyboard(actor),
            )
            .await
        }
        FORBIDDEN_ACCOUNT_DEACTIVATED => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::MainMenu,
                &ui::admin_account_deactivated_text(),
                ui::main_menu_keyboard(actor),
            )
            .await
        }
        FORBIDDEN_SELF_TARGET => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminMenu,
                &ui::admin_self_target_text(),
                ui::admin_menu_keyboard(),
            )
            .await
        }
        LAST_ADMIN_CODE => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminMenu,
                &ui::admin_last_admin_text(),
                ui::admin_menu_keyboard(),
            )
            .await
        }
        USER_NOT_FOUND_CODE => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::AdminMenu,
                &ui::admin_user_not_found_text(),
                ui::admin_menu_keyboard(),
            )
            .await
        }
        _ => send_error(bot, state, chat_id.0, error).await,
    }
}

fn user_display_name(user: &User) -> String {
    let display = user.display_name_object();
    if matches!(
        display.kind(),
        crate::domain::user::DisplayNameKind::Anonymous
    ) {
        return format!("tg id {}", user.telegram_id);
    }
    display.as_str().to_owned()
}

enum ExecutionOutcome {
    RoleChanged(User),
    Deactivated(User),
    Reactivated(User),
}
