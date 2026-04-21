//! `PrincipalContext` — the immutable, per-turn "who & when" that every
//! use-case receives as its **first** argument.
//!
//! Why a dedicated type rather than sprinkling `user_id: i64` / `now: Utc`
//! all over the signatures:
//!
//! * Makes forgetting a policy check structurally harder — the caller has
//!   to construct a context, which forces them to think about identity and
//!   RBAC at the seam.
//! * Freezes `now` for the whole turn; any downstream code that asks the
//!   same `PrincipalContext` for time will see the same value, eliminating
//!   an entire class of "I made two queries and the clock jumped forward"
//!   bugs around SLA and quiet hours.
//! * Carries a trace id so that tracing spans from HTTP → Telegram →
//!   use-case → repo can be correlated without plumbing a string by hand.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::domain::user::UserRole;

/// Identifies *which* inbound Telegram chat this turn originated from.  This
/// is retained so that use-cases producing side effects (like notifications
/// or admin-audit rows) can record the chat even when the actor is acting on
/// behalf of a different user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelegramChatContext {
    pub chat_id: i64,
    pub telegram_user_id: i64,
}

/// The role-shaped identity of whoever initiated the turn.  Always populated
/// for authenticated actions; anonymous / pre-registration flows build a
/// [`PrincipalContext::anonymous`] instead so that RBAC policies can still
/// short-circuit safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalIdentity {
    /// Persistent DB user id; `None` only when the actor has not yet been
    /// registered (first `/start` before onboarding completes).
    pub user_id: Option<i64>,
    pub role: UserRole,
    /// The actor's locale.  Bot is Russian-only as of v3 — this is kept
    /// around for future-proofing but callers must not branch on it unless
    /// the plan explicitly says so.
    pub locale: Locale,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Locale {
    #[default]
    RussianPrimary,
}

/// Immutable per-turn context.
#[derive(Debug, Clone)]
pub struct PrincipalContext {
    pub identity: PrincipalIdentity,
    pub telegram: TelegramChatContext,
    pub trace_id: Uuid,
    /// Frozen "now" for the entire turn.  See the module docs.
    pub now: DateTime<Utc>,
}

impl PrincipalContext {
    pub fn new(
        identity: PrincipalIdentity,
        telegram: TelegramChatContext,
        trace_id: Uuid,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            identity,
            telegram,
            trace_id,
            now,
        }
    }

    /// Builds a context for a pre-registration event where we don't yet have
    /// a DB `user_id`.  Role defaults to `User` so that any policy check
    /// treats this principal as an ordinary, low-privilege actor.
    pub fn anonymous(telegram: TelegramChatContext, trace_id: Uuid, now: DateTime<Utc>) -> Self {
        Self {
            identity: PrincipalIdentity {
                user_id: None,
                role: UserRole::User,
                locale: Locale::default(),
            },
            telegram,
            trace_id,
            now,
        }
    }

    pub fn is_anonymous(&self) -> bool {
        self.identity.user_id.is_none()
    }

    pub fn role(&self) -> UserRole {
        self.identity.role
    }

    pub fn user_id(&self) -> Option<i64> {
        self.identity.user_id
    }

    pub fn telegram_user_id(&self) -> i64 {
        self.telegram.telegram_user_id
    }

    pub fn chat_id(&self) -> i64 {
        self.telegram.chat_id
    }
}

#[cfg(test)]
mod tests {
    use super::{Locale, PrincipalContext, PrincipalIdentity, TelegramChatContext};
    use crate::domain::user::UserRole;
    use chrono::{TimeZone, Utc};
    use uuid::Uuid;

    #[test]
    fn given_anonymous_ctx_when_inspected_then_reports_anonymous_and_user_role() {
        let ctx = PrincipalContext::anonymous(
            TelegramChatContext {
                chat_id: 1,
                telegram_user_id: 42,
            },
            Uuid::nil(),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        );

        assert!(ctx.is_anonymous());
        assert_eq!(ctx.role(), UserRole::User);
        assert_eq!(ctx.telegram_user_id(), 42);
        assert_eq!(ctx.chat_id(), 1);
    }

    #[test]
    fn given_authenticated_ctx_when_inspected_then_returns_persisted_user_id() {
        let ctx = PrincipalContext::new(
            PrincipalIdentity {
                user_id: Some(7),
                role: UserRole::Manager,
                locale: Locale::default(),
            },
            TelegramChatContext {
                chat_id: 1,
                telegram_user_id: 42,
            },
            Uuid::nil(),
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
        );

        assert!(!ctx.is_anonymous());
        assert_eq!(ctx.user_id(), Some(7));
        assert_eq!(ctx.role(), UserRole::Manager);
    }
}
