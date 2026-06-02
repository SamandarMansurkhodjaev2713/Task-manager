//! Registers the bot's command menu with Telegram via `setMyCommands`.
//!
//! # Why this matters
//!
//! When a user opens the "/" input in Telegram they see a list of the bot's
//! available commands.  Without explicit registration that list is either
//! empty or shows stale entries set through BotFather.  By calling
//! `set_my_commands` programmatically at startup we:
//!
//! 1. Always reflect the current command surface — no manual BotFather sync.
//! 2. Show role-appropriate commands: admins and managers see their extra
//!    commands; regular users see only what applies to them.
//! 3. Reduce "command not found" confusion caused by users trying commands
//!    that don't exist for their role.
//!
//! # Scopes used
//!
//! * `Default` — baseline commands for every user who has not been given a
//!   more specific override.  Shown in all private chats and as the fallback
//!   everywhere else.
//!
//! * `ChatMember { chat_id, user_id }` — per-user override for managers and
//!   admins so their extended commands appear immediately after onboarding or
//!   a role change.  In a private chat between the bot and the user
//!   `chat_id == user_id == telegram_id`.
//!
//! # Failure policy
//!
//! Command-menu registration is best-effort.  A Telegram API error here
//! (e.g. unknown bot token in tests, rate-limiting on startup) must never
//! break the main dispatch loop.  All errors are logged as `warn` and
//! silently swallowed so the bot continues to work without a command menu
//! rather than refusing to start.

use teloxide::payloads::SetMyCommandsSetters;
use teloxide::prelude::Requester;
use teloxide::types::{BotCommand, BotCommandScope, UserId};
use teloxide::Bot;

use crate::domain::user::UserRole;

// ─── Command lists ─────────────────────────────────────────────────────────

/// Commands visible to all users.
fn user_commands() -> Vec<BotCommand> {
    vec![
        BotCommand::new("/start", "Запустить или перезапустить бота"),
        BotCommand::new("/menu", "Открыть главное меню"),
        BotCommand::new("/new_task", "Создать задачу"),
        BotCommand::new("/my_tasks", "Мои задачи"),
        BotCommand::new("/created_tasks", "Задачи, созданные мной"),
        BotCommand::new("/status", "Статус задачи по коду (например T-0001)"),
        BotCommand::new("/find", "Найти задачу по тексту"),
        BotCommand::new("/stats", "Моя статистика"),
        BotCommand::new("/settings", "Профиль и настройки"),
        BotCommand::new("/help", "Справка"),
    ]
}

/// Additional commands visible to managers (extends user_commands).
fn manager_extra_commands() -> Vec<BotCommand> {
    vec![
        BotCommand::new("/team_tasks", "Задачи команды"),
        BotCommand::new("/team_stats", "Командная статистика"),
    ]
}

/// Additional commands visible to admins (extends manager_extra_commands).
fn admin_extra_commands() -> Vec<BotCommand> {
    vec![
        BotCommand::new("/admin", "Панель администратора"),
        BotCommand::new(
            "/admin_sync_employees",
            "Синхронизировать справочник сотрудников",
        ),
    ]
}

/// Returns the full command list for the given role.
pub fn commands_for_role(role: UserRole) -> Vec<BotCommand> {
    let mut cmds = user_commands();
    if role.is_manager_or_admin() {
        cmds.extend(manager_extra_commands());
    }
    if role.is_admin() {
        cmds.extend(admin_extra_commands());
    }
    cmds
}

// ─── Registration ───────────────────────────────────────────────────────────

/// Sets the `Default` command scope — the baseline shown to every user
/// who does not have a more specific per-user override.
///
/// Called once at startup.  Errors are logged and swallowed.
pub async fn register_default_commands(bot: &Bot) {
    let commands = user_commands();
    match bot
        .set_my_commands(commands)
        .scope(BotCommandScope::Default)
        .await
    {
        Ok(_) => tracing::info!("bot command menu registered (default scope)"),
        Err(error) => tracing::warn!(
            error = %error,
            "failed to register default bot commands; command menu may be stale"
        ),
    }
}

/// Sets per-user commands for a specific Telegram user based on their role.
///
/// Uses `BotCommandScope::ChatMember` which, for a private bot, means
/// "this specific private chat between bot and user".  For private chats
/// `chat_id == telegram_id`.
///
/// Called after onboarding completion and after admin role changes.
/// Errors are logged as `warn` and swallowed so UX is never interrupted.
pub async fn register_user_commands(bot: &Bot, telegram_id: i64, role: UserRole) {
    let Ok(user_id) = u64::try_from(telegram_id) else {
        tracing::warn!(
            telegram_id,
            "cannot register per-user commands: telegram_id does not fit u64"
        );
        return;
    };

    let commands = commands_for_role(role);
    let scope = BotCommandScope::ChatMember {
        chat_id: teloxide::types::Recipient::Id(teloxide::types::ChatId(telegram_id)),
        user_id: UserId(user_id),
    };

    match bot.set_my_commands(commands).scope(scope).await {
        Ok(_) => tracing::debug!(
            telegram_id,
            role = ?role,
            "per-user bot command menu updated"
        ),
        Err(error) => tracing::warn!(
            telegram_id,
            role = ?role,
            error = %error,
            "failed to update per-user bot commands; command menu may be stale"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{admin_extra_commands, commands_for_role, manager_extra_commands, user_commands};
    use crate::domain::user::UserRole;

    #[test]
    fn given_user_role_when_commands_listed_then_no_admin_or_manager_commands() {
        let cmds = commands_for_role(UserRole::User);
        let cmd_names: Vec<&str> = cmds.iter().map(|c| c.command.as_str()).collect();

        assert!(cmd_names.contains(&"/start"));
        assert!(cmd_names.contains(&"/my_tasks"));
        assert!(cmd_names.contains(&"/help"));
        assert!(
            !cmd_names.contains(&"/admin"),
            "admin command must not appear for regular user"
        );
        assert!(
            !cmd_names.contains(&"/team_tasks"),
            "team_tasks must not appear for regular user"
        );
    }

    #[test]
    fn given_manager_role_when_commands_listed_then_includes_team_commands() {
        let cmds = commands_for_role(UserRole::Manager);
        let cmd_names: Vec<&str> = cmds.iter().map(|c| c.command.as_str()).collect();

        assert!(cmd_names.contains(&"/team_tasks"));
        assert!(cmd_names.contains(&"/team_stats"));
        assert!(
            !cmd_names.contains(&"/admin"),
            "admin command must not appear for manager"
        );
    }

    #[test]
    fn given_admin_role_when_commands_listed_then_includes_admin_commands() {
        let cmds = commands_for_role(UserRole::Admin);
        let cmd_names: Vec<&str> = cmds.iter().map(|c| c.command.as_str()).collect();

        assert!(cmd_names.contains(&"/admin"));
        assert!(cmd_names.contains(&"/admin_sync_employees"));
        assert!(cmd_names.contains(&"/team_tasks"));
    }

    #[test]
    fn given_all_roles_when_user_commands_listed_then_always_present() {
        for role in [UserRole::User, UserRole::Manager, UserRole::Admin] {
            let cmds = commands_for_role(role);
            let cmd_names: Vec<&str> = cmds.iter().map(|c| c.command.as_str()).collect();

            for base in &["/start", "/help", "/my_tasks", "/new_task", "/settings"] {
                assert!(
                    cmd_names.contains(base),
                    "base command {base} must be present for role {role:?}"
                );
            }
        }
    }

    #[test]
    fn given_admin_commands_when_counted_then_user_plus_manager_plus_admin() {
        let total = commands_for_role(UserRole::Admin).len();
        let expected =
            user_commands().len() + manager_extra_commands().len() + admin_extra_commands().len();
        assert_eq!(
            total, expected,
            "admin command list must be the sum of all three sets"
        );
    }
}
