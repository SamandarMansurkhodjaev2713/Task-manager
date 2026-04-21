//! Admin-panel user-facing text (Russian only).
//!
//! All text rendered inside the admin panel is centralised here so that
//! tweaking wording or fixing typos doesn't require touching dispatcher
//! code.  Every function returns an owned `String` to avoid lifetime
//! entanglement with the teloxide reply builders.

use chrono::Utc;

use crate::domain::audit::{AdminAuditEntry, AuditActionCode, SecurityAuditEntry};
use crate::domain::user::{User, UserRole};

use super::super::ui_shared::INFO_EMOJI;

pub fn admin_menu_text(actor: &User) -> String {
    let display = actor.display_name_object();
    let name = if matches!(
        display.kind(),
        crate::domain::user::DisplayNameKind::Anonymous
    ) {
        "администратор".to_owned()
    } else {
        display.as_str().to_owned()
    };
    format!(
        "🛡 Админ-панель\n\nЗдравствуйте, {name}.\n\nВыберите раздел:\n• Администраторы — управление ролями и деактивация\n• Журнал действий — последние изменения, сделанные через панель\n• Флаги функций — переключение экспериментальных возможностей"
    )
}

pub fn admin_access_denied_text() -> String {
    format!(
        "{INFO_EMOJI} Доступ к админ-панели ограничен.\nОбратитесь к действующему администратору, если вам нужен доступ."
    )
}

pub fn admin_account_deactivated_text() -> String {
    format!("{INFO_EMOJI} Ваш аккаунт деактивирован и не может выполнять действия администратора.")
}

pub fn admin_self_target_text() -> String {
    format!(
        "{INFO_EMOJI} Эту операцию нельзя применить к себе.\nПопросите другого администратора выполнить её, чтобы не нарушить инвариант «хотя бы один активный админ»."
    )
}

pub fn admin_last_admin_text() -> String {
    format!(
        "{INFO_EMOJI} Нельзя завершить операцию: вы — последний активный администратор.\nСначала повысьте другого пользователя до роли «Админ», а потом возвращайтесь сюда."
    )
}

pub fn admin_user_not_found_text() -> String {
    format!("{INFO_EMOJI} Пользователь не найден. Возможно, список устарел — вернитесь к списку.")
}

pub fn admin_users_text(users: &[User], actor: &User) -> String {
    if users.is_empty() {
        return format!(
            "{INFO_EMOJI} В системе пока нет активных администраторов, кроме вас ({}).\n\nДобавьте ещё одного, чтобы не потерять доступ при деактивации.",
            actor.telegram_id
        );
    }
    let lines: Vec<String> = users
        .iter()
        .map(|user| {
            let name = display_name(user);
            let role_label = role_label(user.role);
            let status = if user.deactivated_at.is_some() {
                " • ⛔ деактивирован"
            } else {
                ""
            };
            format!(
                "• {name} — {role_label}{status} (tg id {tid})",
                tid = user.telegram_id
            )
        })
        .collect();

    format!(
        "👥 Активные администраторы ({count})\n\n{lines}\n\nВыберите пользователя, чтобы изменить роль или деактивировать.",
        count = users.len(),
        lines = lines.join("\n")
    )
}

pub fn admin_user_details_text(target: &User) -> String {
    let name = display_name(target);
    let role_label = role_label(target.role);
    let username = target
        .telegram_username
        .as_deref()
        .map(|u| format!("@{u}"))
        .unwrap_or_else(|| "не указан".to_owned());
    let status = if let Some(deactivated_at) = target.deactivated_at {
        format!(
            "деактивирован с {}",
            deactivated_at.format("%d.%m.%Y %H:%M")
        )
    } else {
        "активен".to_owned()
    };
    format!(
        "👤 {name}\n\nUsername: {username}\nTelegram ID: {tid}\nРоль: {role_label}\nСтатус: {status}\n\nДля изменения выберите действие ниже.",
        tid = target.telegram_id,
    )
}

pub fn admin_confirm_text(body: &str) -> String {
    format!(
        "⚠️ Подтвердите действие\n\n{body}\n\nНажмите «✅ Подтвердить», чтобы выполнить, или «❌ Отмена», чтобы отказаться.\nКнопки становятся неактивными через 2 минуты."
    )
}

pub fn admin_action_cancelled_text() -> String {
    format!("{INFO_EMOJI} Операция отменена.")
}

pub fn admin_nonce_expired_text() -> String {
    format!(
        "{INFO_EMOJI} Подтверждение не найдено или просрочено.\nОткройте действие заново, чтобы получить новую кнопку подтверждения."
    )
}

pub fn admin_nonce_wrong_actor_text() -> String {
    format!(
        "{INFO_EMOJI} Эта кнопка подтверждения принадлежит другому администратору.\nВыполните действие из своей панели."
    )
}

pub fn admin_role_changed_text(target: &User) -> String {
    let name = display_name(target);
    let role_label = role_label(target.role);
    format!("✅ Роль обновлена\n\n{name} → {role_label}")
}

pub fn admin_deactivated_text(target: &User) -> String {
    let name = display_name(target);
    format!("⛔ Пользователь {name} деактивирован.")
}

pub fn admin_reactivated_text(target: &User) -> String {
    let name = display_name(target);
    format!("✅ Пользователь {name} снова активен.")
}

pub fn admin_audit_text(entries: &[AdminAuditEntry]) -> String {
    if entries.is_empty() {
        return format!("{INFO_EMOJI} Журнал действий пуст.");
    }
    let now = Utc::now();
    let lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            let when = entry.created_at.format("%d.%m %H:%M");
            let age_mins = (now - entry.created_at).num_minutes().max(0);
            let actor = entry
                .actor_user_id
                .map(|id| format!("user#{id}"))
                .unwrap_or_else(|| "platform".to_owned());
            let target = entry
                .target_user_id
                .map(|id| format!("user#{id}"))
                .unwrap_or_else(|| "—".to_owned());
            format!(
                "• {when} ({age}м) {code} • {actor} → {target}",
                age = age_mins,
                code = action_code_ru(entry.action_code)
            )
        })
        .collect();
    format!(
        "📜 Журнал действий (последние {count})\n\n{body}",
        count = entries.len(),
        body = lines.join("\n")
    )
}

pub fn admin_security_audit_text(entries: &[SecurityAuditEntry]) -> String {
    if entries.is_empty() {
        return format!("{INFO_EMOJI} Журнал безопасности пуст.");
    }
    let now = Utc::now();
    let lines: Vec<String> = entries
        .iter()
        .map(|entry| {
            let when = entry.created_at.format("%d.%m %H:%M");
            let age_mins = (now - entry.created_at).num_minutes().max(0);
            let actor = match (entry.actor_user_id, entry.telegram_id) {
                (Some(uid), _) => format!("user#{uid}"),
                (None, Some(tg_id)) => format!("tg#{tg_id}"),
                _ => "—".to_owned(),
            };
            format!(
                "• {when} ({age}м) {code} • {actor}",
                age = age_mins,
                code = security_event_code_ru(entry.event_code),
            )
        })
        .collect();
    format!(
        "🔐 Журнал безопасности (последние {count})\n\n{body}\n\n⚠️ Этот журнал содержит попытки отказа в доступе, нарушения авторства коллбэков и ограничения по rate-limit.",
        count = entries.len(),
        body = lines.join("\n")
    )
}

fn security_event_code_ru(code: AuditActionCode) -> &'static str {
    match code {
        AuditActionCode::ForbiddenActionAttempted => "запрещённое действие",
        AuditActionCode::CallbackAuthorshipViolation => "чужой callback",
        AuditActionCode::RateLimitExceeded => "rate-limit",
        AuditActionCode::AdminNonceExpired => "nonce просрочен",
        // Admin-side codes that shouldn't appear in the security log but are
        // included for exhaustiveness.
        _ => action_code_ru(code),
    }
}

/// Renders the feature-flags admin screen.
///
/// `flags` is `all_flags()` from the live [`FeatureFlagRegistry`]; each
/// entry is `(flag, currently_enabled)`.  Changes are effective immediately
/// and survive process restarts via the `feature_flag_overrides` table.
pub fn admin_features_text(flags: &[(crate::shared::feature_flags::FeatureFlag, bool)]) -> String {
    let mut lines = vec!["🚩 Флаги функций".to_owned(), String::new()];
    for (flag, enabled) in flags {
        let mark = if *enabled { "✅" } else { "⬜" };
        lines.push(format!("{mark} {}", flag.as_key()));
    }
    lines.push(String::new());
    lines.push(
        "Нажмите на флаг, чтобы переключить его. Изменения применяются немедленно и сохраняются после перезапуска.".to_owned(),
    );
    lines.join("\n")
}

fn display_name(user: &User) -> String {
    let display = user.display_name_object();
    if matches!(
        display.kind(),
        crate::domain::user::DisplayNameKind::Anonymous
    ) {
        return format!("tg id {}", user.telegram_id);
    }
    display.as_str().to_owned()
}

fn role_label(role: UserRole) -> &'static str {
    match role {
        UserRole::User => "Пользователь",
        UserRole::Manager => "Менеджер",
        UserRole::Admin => "Админ",
    }
}

fn action_code_ru(code: AuditActionCode) -> &'static str {
    match code {
        AuditActionCode::UserOnboardingStarted => "онбординг: старт",
        AuditActionCode::UserOnboardingCompleted => "онбординг: завершён",
        AuditActionCode::UserOnboardingAbandoned => "онбординг: прерван",
        AuditActionCode::UserEmployeeLinked => "сотрудник: связь",
        AuditActionCode::UserEmployeeUnlinked => "сотрудник: отвязка",
        AuditActionCode::RoleElevatedByBootstrap => "админ: bootstrap",
        AuditActionCode::RoleChangedByAdmin => "роль: изменение",
        AuditActionCode::UserDeactivatedByAdmin => "пользователь: деактивация",
        AuditActionCode::UserReactivatedByAdmin => "пользователь: реактивация",
        AuditActionCode::AdminFeatureToggled => "флаг: переключение",
        AuditActionCode::ForbiddenActionAttempted => "отказ в доступе",
        AuditActionCode::CallbackAuthorshipViolation => "чужой callback",
        AuditActionCode::RateLimitExceeded => "rate-limit",
        AuditActionCode::AdminNonceExpired => "nonce просрочен",
    }
}
