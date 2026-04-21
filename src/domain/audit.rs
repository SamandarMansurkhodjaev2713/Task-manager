use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: Option<i64>,
    pub task_id: i64,
    pub action: AuditAction,
    pub old_status: Option<String>,
    pub new_status: Option<String>,
    pub changed_by_user_id: Option<i64>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Created,
    Sent,
    Assigned,
    StatusChanged,
    ReviewRequested,
    Reassigned,
    Blocked,
    Commented,
    Edited,
    Cancelled,
    EmployeesSynced,
}

// ─── Admin / security audit (RBAC, onboarding, privileged actions) ─────────

/// Sealed enum of every event that can appear in either the admin or the
/// security audit log.  Centralising these as a type (rather than free-form
/// strings) guarantees dashboards, runbooks, and alerts agree on vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditActionCode {
    // Registration / onboarding
    UserOnboardingStarted,
    UserOnboardingCompleted,
    UserOnboardingAbandoned,
    UserEmployeeLinked,
    UserEmployeeUnlinked,

    // RBAC
    RoleElevatedByBootstrap,
    RoleChangedByAdmin,
    UserDeactivatedByAdmin,
    UserReactivatedByAdmin,
    AdminFeatureToggled,

    // Security / access
    ForbiddenActionAttempted,
    CallbackAuthorshipViolation,
    RateLimitExceeded,
    AdminNonceExpired,
}

impl AuditActionCode {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::UserOnboardingStarted => "user_onboarding_started",
            Self::UserOnboardingCompleted => "user_onboarding_completed",
            Self::UserOnboardingAbandoned => "user_onboarding_abandoned",
            Self::UserEmployeeLinked => "user_employee_linked",
            Self::UserEmployeeUnlinked => "user_employee_unlinked",
            Self::RoleElevatedByBootstrap => "role_elevated_by_bootstrap",
            Self::RoleChangedByAdmin => "role_changed_by_admin",
            Self::UserDeactivatedByAdmin => "user_deactivated_by_admin",
            Self::UserReactivatedByAdmin => "user_reactivated_by_admin",
            Self::AdminFeatureToggled => "admin_feature_toggled",
            Self::ForbiddenActionAttempted => "forbidden_action_attempted",
            Self::CallbackAuthorshipViolation => "callback_authorship_violation",
            Self::RateLimitExceeded => "rate_limit_exceeded",
            Self::AdminNonceExpired => "admin_nonce_expired",
        }
    }
}

/// A row that will land in `admin_audit_log` (migration 008).  Captures
/// privileged state transitions — role changes, user activation, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdminAuditEntry {
    pub id: Option<i64>,
    /// `None` means the action was performed by the platform itself (bootstrap).
    pub actor_user_id: Option<i64>,
    pub target_user_id: Option<i64>,
    pub action_code: AuditActionCode,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

/// A row that will land in `security_audit_log` (migration 008).  Captures
/// attempted-but-denied / suspicious events — forbidden actions, callback
/// forgery, rate-limit storms.  These are intentionally separate from the
/// admin log to keep forensics noise out of the management UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAuditEntry {
    pub id: Option<i64>,
    pub actor_user_id: Option<i64>,
    pub telegram_id: Option<i64>,
    pub event_code: AuditActionCode,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}
