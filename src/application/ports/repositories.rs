use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use std::collections::HashMap;
use uuid::Uuid;

use crate::domain::audit::{AdminAuditEntry, AuditLogEntry, SecurityAuditEntry};
use crate::domain::comment::TaskComment;
use crate::domain::employee::Employee;
use crate::domain::errors::AppResult;
use crate::domain::notification::{Notification, NotificationType};
use crate::domain::task::{Task, TaskStats};
use crate::domain::user::{User, UserRole};
use crate::domain::voice::{VoiceProcessingRecord, VoiceProcessingState};
use crate::shared::feature_flags::FeatureFlag;

#[derive(Debug, Clone)]
pub enum PersistedTask {
    Created(Task),
    Existing(Task),
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn upsert_from_message(&self, user: &User) -> AppResult<User>;
    async fn find_by_id(&self, user_id: i64) -> AppResult<Option<User>>;
    async fn find_by_telegram_id(&self, telegram_id: i64) -> AppResult<Option<User>>;
    async fn find_by_username(&self, username: &str) -> AppResult<Option<User>>;
    async fn list_with_chat_id(&self) -> AppResult<Vec<User>>;

    /// Creates or resumes an onboarding session for the given Telegram user.
    /// Called at the very first `/start` — does NOT populate first/last name.
    async fn ensure_onboarding_session(
        &self,
        telegram_id: i64,
        chat_id: i64,
        telegram_username: Option<&str>,
        fallback_full_name: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<User>;

    /// Persists the current FSM step.  Uses optimistic concurrency on
    /// `onboarding_version` — returns `OnboardingConcurrencyConflict` error
    /// when another update already advanced the state.
    async fn save_onboarding_progress(
        &self,
        user_id: i64,
        expected_version: i64,
        next_state: crate::domain::user::OnboardingState,
        first_name: Option<&str>,
        last_name: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<User>;

    /// Marks onboarding as completed and (optionally) links the user to an
    /// employee record.  Also writes the canonical `full_name` for backward
    /// compatibility with existing UI.
    async fn complete_onboarding(
        &self,
        user_id: i64,
        expected_version: i64,
        first_name: &str,
        last_name: &str,
        linked_employee_id: Option<i64>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<User>;

    // ── RBAC / admin operations ────────────────────────────────────────────────

    /// Sets a user's role.  Must enforce the last-admin invariant: demoting
    /// the only remaining **active** admin (i.e. role=Admin AND
    /// `deactivated_at IS NULL`) must return
    /// [`AppError::business_rule`] with code `LAST_ADMIN_PROTECTED` so that
    /// bootstrap remains recoverable.  The caller (policy + use case) is
    /// expected to have already authorized the action and to pass an
    /// up-to-date `expected_updated_at` for optimistic concurrency.
    async fn set_role(
        &self,
        actor_user_id: i64,
        target_user_id: i64,
        new_role: UserRole,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<User>;

    /// Soft-deactivates a user (sets `deactivated_at`).  Must enforce the
    /// last-admin invariant the same way as [`set_role`].
    async fn deactivate(
        &self,
        actor_user_id: i64,
        target_user_id: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<User>;

    /// Reactivates a previously deactivated user.  Idempotent.
    async fn reactivate(
        &self,
        actor_user_id: i64,
        target_user_id: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<User>;

    /// Lists all currently active admins.  Used by the last-admin invariant
    /// checks and by the admin panel's "ops" screen.
    async fn list_active_admins(&self) -> AppResult<Vec<User>>;

    /// Idempotent bootstrap helper: promotes the given Telegram ID to admin
    /// **only if** the user already exists in the database.  Missing users
    /// are not auto-created — bootstrap only runs after a user has sent at
    /// least one `/start`.  Returns `None` when the Telegram ID has not yet
    /// registered, and `Some(user)` in all other cases (already-admin rows
    /// are returned unchanged so the caller can distinguish elevation from
    /// no-op).
    async fn promote_bootstrap_admin(
        &self,
        telegram_id: i64,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Option<BootstrapPromotion>>;
}

/// Result of [`UserRepository::promote_bootstrap_admin`].  Encodes whether
/// the user was elevated now (`Elevated`) or was already an admin from a
/// prior startup (`AlreadyAdmin`) so that the caller can decide whether to
/// emit an audit record.
#[derive(Debug, Clone)]
pub enum BootstrapPromotion {
    Elevated(User),
    AlreadyAdmin(User),
}

/// Append-only log of privileged actions (role changes, user
/// (de)activations, feature flag toggles).  Read by the admin panel and by
/// the operations runbook.
#[async_trait]
pub trait AdminAuditLogRepository: Send + Sync {
    async fn append(&self, entry: &AdminAuditEntry) -> AppResult<AdminAuditEntry>;
    async fn list_recent(&self, limit: i64) -> AppResult<Vec<AdminAuditEntry>>;
    async fn list_for_target(
        &self,
        target_user_id: i64,
        limit: i64,
    ) -> AppResult<Vec<AdminAuditEntry>>;
}

/// Append-only log of security-sensitive **attempts** — forbidden
/// operations, callback authorship mismatches, rate-limit storms.  Kept
/// separate from [`AdminAuditLogRepository`] so retention and access
/// policies can differ.
#[async_trait]
pub trait SecurityAuditLogRepository: Send + Sync {
    async fn append(&self, entry: &SecurityAuditEntry) -> AppResult<SecurityAuditEntry>;
    async fn list_recent(&self, limit: i64) -> AppResult<Vec<SecurityAuditEntry>>;
}

#[async_trait]
pub trait EmployeeRepository: Send + Sync {
    async fn upsert_many(&self, employees: &[Employee]) -> AppResult<usize>;
    async fn list_active(&self) -> AppResult<Vec<Employee>>;
    async fn find_by_id(&self, employee_id: i64) -> AppResult<Option<Employee>>;
}

/// Identifies the kind of directory row that owns a set of person trigrams.
///
/// This is mirrored 1:1 in `migrations/009_person_trigrams_and_idempotency.sql`
/// via the `owner_kind` CHECK constraint — changes here must be paired with a
/// migration update and a cascade plan, hence the `#[non_exhaustive]` on a
/// closed domain enum is deliberately avoided: we want the compiler to flag
/// every call site when a new owner kind is introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PersonTrigramOwnerKind {
    Employee,
    User,
}

impl PersonTrigramOwnerKind {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Employee => "employee",
            Self::User => "user",
        }
    }
}

/// One owner's fuzzy-match candidate and its similarity score (0..=1).
///
/// The score here is computed by the adapter: we expose it as an `f32`
/// (rather than opaque ranks) so callers can threshold it and combine
/// multiple signals (e.g. exact-prefix boosts for the onboarding flow).
#[derive(Debug, Clone, PartialEq)]
pub struct PersonTrigramCandidate {
    pub owner_kind: PersonTrigramOwnerKind,
    pub owner_id: i64,
    pub shared_trigrams: u32,
    pub score: f32,
}

/// Fuzzy-match index over person names.  Skeleton port for Phase 5.
///
/// The concrete adapter is `SqlitePersonTrigramIndex`, which stores trigrams
/// in `person_trigrams` (migration 009).  Higher-level use-cases (assignee
/// clarification, /find) will consume this trait so they remain agnostic of
/// the backing storage.
#[async_trait]
pub trait PersonTrigramIndex: Send + Sync {
    /// Replace the trigram set for `(owner_kind, owner_id)` with `trigrams`.
    /// Idempotent: callers may re-submit the same set without duplicates.
    async fn upsert(
        &self,
        owner_kind: PersonTrigramOwnerKind,
        owner_id: i64,
        trigrams: &[String],
    ) -> AppResult<()>;

    /// Delete all trigrams for an owner, used when a user merges or an
    /// employee is tombstoned in the directory.
    async fn delete(&self, owner_kind: PersonTrigramOwnerKind, owner_id: i64) -> AppResult<()>;

    /// Rank owners by trigram overlap with `query_trigrams`.
    /// `limit` is clamped to `[1, 50]` by the adapter.
    async fn top_matches(
        &self,
        query_trigrams: &[String],
        limit: u32,
    ) -> AppResult<Vec<PersonTrigramCandidate>>;
}

#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create_if_absent(&self, task: &Task) -> AppResult<PersistedTask>;
    async fn find_by_id(&self, task_id: i64) -> AppResult<Option<Task>>;
    async fn find_by_uid(&self, task_uid: Uuid) -> AppResult<Option<Task>>;
    async fn update(&self, task: &Task) -> AppResult<Task>;
    async fn list_assigned_to_user(
        &self,
        user_id: i64,
        cursor: Option<String>,
        limit: u32,
    ) -> AppResult<Vec<Task>>;
    async fn list_open_assigned_to_employee_without_user(
        &self,
        employee_id: i64,
        limit: i64,
    ) -> AppResult<Vec<Task>>;
    async fn list_created_by_user(
        &self,
        user_id: i64,
        cursor: Option<String>,
        limit: u32,
    ) -> AppResult<Vec<Task>>;
    async fn list_all(&self, cursor: Option<String>, limit: u32) -> AppResult<Vec<Task>>;
    async fn get_due_between(
        &self,
        start: NaiveDate,
        end: NaiveDate,
        limit: i64,
    ) -> AppResult<Vec<Task>>;
    async fn get_overdue(&self, as_of: NaiveDate, limit: i64) -> AppResult<Vec<Task>>;
    async fn count_stats_for_user(&self, user_id: i64) -> AppResult<TaskStats>;
    async fn count_stats_global(&self) -> AppResult<TaskStats>;
    async fn list_open(&self, limit: i64) -> AppResult<Vec<Task>>;
}

#[async_trait]
pub trait NotificationRepository: Send + Sync {
    async fn enqueue(&self, notification: &Notification) -> AppResult<Notification>;
    async fn list_pending(&self, limit: i64) -> AppResult<Vec<Notification>>;
    async fn mark_sent(
        &self,
        notification_id: i64,
        telegram_message_id: i32,
        sent_at_utc: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<()>;
    async fn mark_retry_pending(
        &self,
        notification_id: i64,
        next_attempt_at: chrono::DateTime<chrono::Utc>,
        error_code: &'static str,
    ) -> AppResult<()>;
    async fn mark_failed(&self, notification_id: i64, error_code: &'static str) -> AppResult<()>;
    async fn requeue(&self, notification_id: i64) -> AppResult<()>;
    async fn find_latest_for_task_and_recipient(
        &self,
        task_id: i64,
        recipient_user_id: i64,
        notification_type: NotificationType,
    ) -> AppResult<Option<Notification>>;
}

#[async_trait]
pub trait AuditLogRepository: Send + Sync {
    async fn append(&self, entry: &AuditLogEntry) -> AppResult<AuditLogEntry>;
    async fn list_for_task(&self, task_id: i64) -> AppResult<Vec<AuditLogEntry>>;
}

#[async_trait]
pub trait CommentRepository: Send + Sync {
    async fn create(&self, comment: &TaskComment) -> AppResult<TaskComment>;
    async fn list_recent_for_task(&self, task_id: i64, limit: i64) -> AppResult<Vec<TaskComment>>;
}

/// CAS (compare-and-swap) state-transition outcome for voice-processing rows.
///
/// The caller distinguishes between a successful transition and a stale
/// attempt (another worker already moved the row forward) without needing
/// to reload the full record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceTransitionOutcome {
    Transitioned(VoiceProcessingRecord),
    StaleExpectedState,
    NotFound,
    InvalidTransition,
}

/// Voice-to-task persistence port (Phase 6 skeleton).
///
/// The adapter backs this with `voice_processing_records` (migration 010).
/// The port models the full attempt lifecycle so workers can:
/// * deduplicate by `file_unique_id`,
/// * record progress atomically with CAS,
/// * and never observe a half-applied transition.
#[async_trait]
pub trait VoiceProcessingRepository: Send + Sync {
    /// Inserts a brand-new `queued` row if none exists for this
    /// `file_unique_id`.  Returns the (possibly existing) record so callers
    /// can short-circuit retries.
    async fn get_or_create_queued(
        &self,
        record: &VoiceProcessingRecord,
    ) -> AppResult<VoiceProcessingRecord>;

    async fn find_by_file_unique_id(
        &self,
        file_unique_id: &str,
    ) -> AppResult<Option<VoiceProcessingRecord>>;

    /// Compare-and-swap transition: succeeds only if the row currently has
    /// `expected` state.  Increments `attempt_count` and refreshes
    /// `updated_at` on success.
    async fn transition_state(
        &self,
        file_unique_id: &str,
        expected: VoiceProcessingState,
        next: VoiceProcessingState,
        error_code: Option<&str>,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<VoiceTransitionOutcome>;

    /// Records a successful transcription, including the preview hash used
    /// to avoid echoing raw content back into logs.
    async fn mark_transcribed(
        &self,
        file_unique_id: &str,
        transcript_preview_hash: &str,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<VoiceTransitionOutcome>;

    /// Scrubs transcript-derived payloads on records that completed
    /// more than `older_than` ago.  Keeps the row itself so retried
    /// webhooks stay idempotent, but removes `transcript_preview_hash`
    /// and `last_error_code` (when it contained user-facing details).
    /// Returns the number of rows scrubbed.
    ///
    /// Implementations MUST be idempotent and never touch rows whose
    /// state is not terminal.
    async fn purge_stale_payloads(
        &self,
        older_than: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<u64>;
}

// ── SLA escalation repository ─────────────────────────────────────────────

/// A lightweight projection of an active task used by the SLA escalation
/// worker.  Avoids loading the full `Task` aggregate for a background scan
/// that only touches deadline-related columns.
#[derive(Debug, Clone)]
pub struct SlaTaskRow {
    pub id: i64,
    pub task_uid: Uuid,
    pub title: String,
    pub deadline: NaiveDate,
    pub created_at: DateTime<Utc>,
    pub assigned_to_user_id: Option<i64>,
    /// Current persisted SLA state code (`"healthy"` / `"at_risk"` /
    /// `"breached"`).  `None` means the column was never written.
    pub current_sla_state: Option<String>,
    /// Last escalation level recorded for this task (0 = none fired yet).
    pub sla_last_level: i32,
}

/// Write-side port for SLA state tracking.  Writes are intentionally narrow
/// so the worker can update only the two SLA columns without touching the
/// rest of the task aggregate.
#[async_trait]
pub trait SlaRepository: Send + Sync {
    /// Returns tasks that have a concrete deadline and are not yet terminal.
    /// Ordered by deadline ascending so the most-urgent tasks are visited
    /// first.
    async fn list_active_with_deadline(&self, limit: i64) -> AppResult<Vec<SlaTaskRow>>;

    /// Overwrites `sla_state` and `sla_last_level` for one task row and
    /// bumps `updated_at`.
    async fn update_sla_state(
        &self,
        task_id: i64,
        state: &str,
        last_level: i32,
        now: DateTime<Utc>,
    ) -> AppResult<()>;

    /// Inserts into `sla_escalations` with `INSERT OR IGNORE` semantics.
    /// Returns `true` when the row was newly created; `false` when it
    /// already existed for `(task_id, level)` — so the caller can avoid
    /// double-sending notifications.
    async fn record_escalation(
        &self,
        task_id: i64,
        level: i32,
        actor: &str,
        detail: serde_json::Value,
        now: DateTime<Utc>,
    ) -> AppResult<bool>;
}

// ── Recurrence rule repository ────────────────────────────────────────────

/// Lightweight projection of a `recurrence_rules` row for the scheduler.
#[derive(Debug, Clone)]
pub struct RecurrenceRuleRow {
    pub id: i64,
    pub template_id: Option<i64>,
    pub owner_user_id: i64,
    pub cron_expression: String,
    pub timezone: String,
}

/// Lightweight projection of a `task_templates` row.
#[derive(Debug, Clone)]
pub struct TaskTemplateRow {
    pub id: i64,
    pub code: String,
    pub title: String,
    /// Raw JSON body — decode with
    /// [`decode_template_body`][crate::domain::recurrence::decode_template_body].
    pub body: String,
    pub created_by_user_id: Option<i64>,
}

/// Read/write port for the recurrence-rule scheduler.
#[async_trait]
pub trait RecurrenceRepository: Send + Sync {
    /// Returns active rules whose `next_run_at <= as_of`, ordered ascending.
    async fn list_due(&self, as_of: DateTime<Utc>, limit: i64)
        -> AppResult<Vec<RecurrenceRuleRow>>;

    /// Loads a single active template by primary key.  Returns `None` when
    /// the template has been deleted or deactivated.
    async fn get_template(&self, template_id: i64) -> AppResult<Option<TaskTemplateRow>>;

    /// After firing, advance the rule's schedule: write `last_run_at` and
    /// the recomputed `next_run_at`.
    async fn advance_rule(
        &self,
        rule_id: i64,
        fired_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> AppResult<()>;
}

/// Persistence port for runtime feature flag overrides written by the admin
/// panel.  The canonical truth at runtime is the in-memory
/// [`SharedFeatureFlagRegistry`][crate::shared::feature_flags::SharedFeatureFlagRegistry];
/// this repository backs that registry across process restarts.
///
/// The underlying table is `feature_flag_overrides` (migration 007).
#[async_trait]
pub trait FeatureFlagRepository: Send + Sync {
    /// Returns all persisted flag overrides as a `{flag → enabled}` map.
    /// Rows whose `flag_key` no longer maps to a known [`FeatureFlag`] variant
    /// are silently skipped (forward-compatibility with older DB schemas).
    async fn list_overrides(&self) -> AppResult<HashMap<FeatureFlag, bool>>;

    /// Upserts a single flag override.  Passing `enabled = true` turns the
    /// flag on; `false` turns it off.  The write is unconditional — it always
    /// supersedes any previous value for the same `flag_key`.
    async fn upsert_override(
        &self,
        flag: FeatureFlag,
        enabled: bool,
        updated_by_user_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> AppResult<()>;
}
