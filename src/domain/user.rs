use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::errors::AppError;
use crate::domain::message::IncomingMessage;
use crate::domain::notification_preferences::NotificationPreferences;

/// Default fleet-wide timezone.  v3 is Russia-only — see the plan.
pub const DEFAULT_USER_TIMEZONE: &str = "Europe/Moscow";
pub const DEFAULT_QUIET_HOURS_START_MIN: i32 = 22 * 60;
pub const DEFAULT_QUIET_HOURS_END_MIN: i32 = 8 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    User,
    Manager,
    Admin,
}

impl UserRole {
    pub fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }

    pub fn is_manager_or_admin(self) -> bool {
        matches!(self, Self::Manager | Self::Admin)
    }
}

impl Display for UserRole {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::User => "user",
            Self::Manager => "manager",
            Self::Admin => "admin",
        };
        formatter.write_str(value)
    }
}

/// FSM state for the onboarding flow (see `OnboardingFsm` in the application
/// layer).  Stored on `users.onboarding_state`.  A `None` in the DB maps to
/// `OnboardingState::Completed` for legacy rows so that existing accounts
/// don't get forced back through onboarding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnboardingState {
    AwaitingFirstName,
    AwaitingLastName,
    AwaitingEmployeeLink,
    Completed,
}

impl OnboardingState {
    pub fn as_storage_value(self) -> &'static str {
        match self {
            Self::AwaitingFirstName => "awaiting_first_name",
            Self::AwaitingLastName => "awaiting_last_name",
            Self::AwaitingEmployeeLink => "awaiting_employee_link",
            Self::Completed => "completed",
        }
    }

    pub fn from_storage_value(value: Option<&str>) -> Self {
        match value {
            Some("awaiting_first_name") => Self::AwaitingFirstName,
            Some("awaiting_last_name") => Self::AwaitingLastName,
            Some("awaiting_employee_link") => Self::AwaitingEmployeeLink,
            Some("completed") | None => Self::Completed,
            // Unknown values are treated as "completed" to avoid locking legacy
            // users out if a release rolls back the state vocabulary.
            Some(_) => Self::Completed,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Option<i64>,
    pub telegram_id: i64,
    pub last_chat_id: Option<i64>,
    pub telegram_username: Option<String>,
    pub full_name: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub linked_employee_id: Option<i64>,
    pub is_employee: bool,
    pub role: UserRole,
    pub onboarding_state: OnboardingState,
    pub onboarding_version: i64,
    pub timezone: String,
    pub quiet_hours_start_min: i32,
    pub quiet_hours_end_min: i32,
    pub deactivated_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Source-of-truth name rendering for the UI.
///
/// A [`DisplayName`] is always produced from a [`User`] via
/// [`User::display_name_object`] so that every renderer (welcome banner, task
/// card, audit line) agrees on:
///
/// * **Priority:** canonical `first_name + last_name` ≫ `first_name` alone ≫
///   Telegram `full_name` ≫ `@username` ≫ literal `"Пользователь"`.
/// * **Safety:** we never render a user-controlled `full_name` *after* the
///   user has gone through onboarding, because that field is initially
///   populated from the Telegram display name (which can be anything — see
///   the P0 screenshot where the welcome banner said
///   `"Добро пожаловать, Killallofthem!"`).  Using the onboarding-supplied
///   first/last pair defeats that drift.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayName {
    value: String,
    kind: DisplayNameKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayNameKind {
    /// Both first and last name came from onboarding.  Safe for all UX.
    FullOnboarded,
    /// Only the first name is known (onboarding was interrupted).
    FirstOnly,
    /// Fallback to the Telegram display name (pre-onboarding users).
    TelegramFullName,
    /// Fallback to the Telegram username (if display name is missing).
    TelegramUsername,
    /// Literal fallback when nothing better is available.
    Anonymous,
}

impl DisplayName {
    pub fn as_str(&self) -> &str {
        &self.value
    }

    pub fn kind(&self) -> DisplayNameKind {
        self.kind
    }

    /// True when the name came from the onboarding flow (first + last).
    pub fn is_onboarded(&self) -> bool {
        matches!(self.kind, DisplayNameKind::FullOnboarded)
    }
}

impl std::fmt::Display for DisplayName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.value)
    }
}

impl User {
    pub fn from_message(message: &IncomingMessage, role: UserRole, is_employee: bool) -> Self {
        let now = message.timestamp;
        Self {
            id: None,
            telegram_id: message.sender_id,
            last_chat_id: Some(message.chat_id),
            telegram_username: message.sender_username.clone(),
            full_name: Some(message.sender_name.clone()),
            first_name: None,
            last_name: None,
            linked_employee_id: None,
            is_employee,
            role,
            onboarding_state: OnboardingState::Completed,
            onboarding_version: 0,
            timezone: DEFAULT_USER_TIMEZONE.to_owned(),
            quiet_hours_start_min: DEFAULT_QUIET_HOURS_START_MIN,
            quiet_hours_end_min: DEFAULT_QUIET_HOURS_END_MIN,
            deactivated_at: None,
            created_at: now,
            updated_at: now,
        }
    }

    /// Legacy string-returning accessor.  New code should prefer
    /// [`User::display_name_object`] so we preserve the provenance of the
    /// name and can emit appropriate fallbacks.  Implemented in terms of the
    /// value object to keep them in sync.
    pub fn display_name(&self) -> String {
        self.display_name_object().value
    }

    /// Value-object projection of the user's name.  See [`DisplayName`] for
    /// the priority rules.
    pub fn display_name_object(&self) -> DisplayName {
        if let (Some(first), Some(last)) = (self.first_name.as_deref(), self.last_name.as_deref()) {
            if !first.is_empty() && !last.is_empty() {
                return DisplayName {
                    value: format!("{first} {last}"),
                    kind: DisplayNameKind::FullOnboarded,
                };
            }
        }
        if let Some(first) = self.first_name.as_deref() {
            if !first.is_empty() {
                return DisplayName {
                    value: first.to_owned(),
                    kind: DisplayNameKind::FirstOnly,
                };
            }
        }
        if let Some(full) = self.full_name.as_deref() {
            if !full.is_empty() {
                return DisplayName {
                    value: full.to_owned(),
                    kind: DisplayNameKind::TelegramFullName,
                };
            }
        }
        if let Some(username) = self.telegram_username.as_deref() {
            if !username.is_empty() {
                return DisplayName {
                    value: format!("@{username}"),
                    kind: DisplayNameKind::TelegramUsername,
                };
            }
        }
        DisplayName {
            value: "Пользователь".to_owned(),
            kind: DisplayNameKind::Anonymous,
        }
    }

    pub fn is_onboarded(&self) -> bool {
        self.onboarding_state.is_terminal()
            && self.first_name.as_deref().is_some_and(|v| !v.is_empty())
            && self.last_name.as_deref().is_some_and(|v| !v.is_empty())
    }

    /// Derives a [`NotificationPreferences`] value object from the
    /// persistent user columns.  Returns a validation error if any column
    /// (timezone code, quiet-hours minutes) is out of range, which would
    /// indicate a corrupt row — in that case the caller is expected to
    /// surface a friendly error and *not* silently suppress notifications.
    pub fn notification_preferences(&self) -> Result<NotificationPreferences, AppError> {
        NotificationPreferences::new(
            &self.timezone,
            self.quiet_hours_start_min,
            self.quiet_hours_end_min,
        )
    }
}
