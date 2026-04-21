//! Regression tests for the canonical [`User::display_name_object`].
//!
//! The fleet's cardinal UX drift came from `actor.full_name` being rendered
//! *after* onboarding, which let raw Telegram display names leak into the
//! welcome banner (screenshot P0: "Добро пожаловать, Killallofthem!").  These
//! tests fence in the priority: onboarded FIO ≫ first-only ≫ telegram_full
//! ≫ @username ≫ "Пользователь", and verify each fallback has an observable
//! `kind()`.

use chrono::Utc;
use telegram_task_bot::domain::user::{
    DisplayNameKind, OnboardingState, User, UserRole, DEFAULT_QUIET_HOURS_END_MIN,
    DEFAULT_QUIET_HOURS_START_MIN, DEFAULT_USER_TIMEZONE,
};

fn base_user() -> User {
    User {
        id: Some(1),
        telegram_id: 100,
        last_chat_id: Some(100),
        telegram_username: None,
        full_name: None,
        first_name: None,
        last_name: None,
        linked_employee_id: None,
        is_employee: false,
        role: UserRole::User,
        onboarding_state: OnboardingState::Completed,
        onboarding_version: 1,
        timezone: DEFAULT_USER_TIMEZONE.to_owned(),
        quiet_hours_start_min: DEFAULT_QUIET_HOURS_START_MIN,
        quiet_hours_end_min: DEFAULT_QUIET_HOURS_END_MIN,
        deactivated_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

#[test]
fn given_onboarded_user_when_display_then_uses_first_plus_last() {
    let mut user = base_user();
    user.first_name = Some("Иван".into());
    user.last_name = Some("Иванов".into());
    user.full_name = Some("Killallofthem".into());
    user.telegram_username = Some("ivanov".into());

    let display = user.display_name_object();

    assert_eq!(display.as_str(), "Иван Иванов");
    assert_eq!(display.kind(), DisplayNameKind::FullOnboarded);
    assert!(display.is_onboarded());
}

#[test]
fn given_only_first_name_when_display_then_falls_back_to_first_only() {
    let mut user = base_user();
    user.first_name = Some("Иван".into());
    user.full_name = Some("Killallofthem".into());

    let display = user.display_name_object();

    assert_eq!(display.as_str(), "Иван");
    assert_eq!(display.kind(), DisplayNameKind::FirstOnly);
    assert!(!display.is_onboarded());
}

#[test]
fn given_empty_first_and_last_when_display_then_uses_telegram_full_name() {
    let mut user = base_user();
    user.first_name = Some(String::new());
    user.last_name = Some(String::new());
    user.full_name = Some("Killallofthem".into());

    let display = user.display_name_object();

    assert_eq!(display.as_str(), "Killallofthem");
    assert_eq!(display.kind(), DisplayNameKind::TelegramFullName);
}

#[test]
fn given_only_username_when_display_then_prefixes_with_at_sign() {
    let mut user = base_user();
    user.telegram_username = Some("ivanov".into());

    let display = user.display_name_object();

    assert_eq!(display.as_str(), "@ivanov");
    assert_eq!(display.kind(), DisplayNameKind::TelegramUsername);
}

#[test]
fn given_no_name_sources_when_display_then_anonymous_literal() {
    let user = base_user();

    let display = user.display_name_object();

    assert_eq!(display.as_str(), "Пользователь");
    assert_eq!(display.kind(), DisplayNameKind::Anonymous);
}

#[test]
fn given_onboarded_user_with_noise_full_name_when_display_then_full_name_is_ignored() {
    // Reproduces the drift bug directly: user finished onboarding as "Иван
    // Иванов", but Telegram display name is still noise; welcome_text must
    // never prefer the noise over the canonical FIO.
    let mut user = base_user();
    user.first_name = Some("Иван".into());
    user.last_name = Some("Иванов".into());
    user.full_name = Some("Killallofthem".into());

    let display = user.display_name_object();

    assert_eq!(display.as_str(), "Иван Иванов");
    assert_ne!(display.as_str(), "Killallofthem");
}

#[test]
fn given_legacy_display_name_accessor_when_called_then_matches_display_name_object() {
    let mut user = base_user();
    user.first_name = Some("Иван".into());
    user.last_name = Some("Иванов".into());

    assert_eq!(user.display_name(), user.display_name_object().as_str());
}
