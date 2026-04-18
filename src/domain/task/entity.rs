use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::domain::errors::{AppError, AppResult};
use crate::domain::task::draft::StructuredTaskDraft;
use crate::domain::task::types::{MessageType, TaskPriority, TaskStatus};
use crate::shared::constants::limits::MAX_TASK_BLOCKER_REASON_LENGTH;

/// The central aggregate for a work item in the system.
///
/// All mutations return a new `Task` value — the original is never modified in place.
/// Every mutating operation increments `version` to support optimistic locking in the
/// persistence layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: Option<i64>,
    pub task_uid: Uuid,
    pub version: i64,
    pub source_message_key: String,
    pub created_by_user_id: i64,
    pub assigned_to_user_id: Option<i64>,
    pub assigned_to_employee_id: Option<i64>,
    pub title: String,
    pub description: String,
    pub acceptance_criteria: Vec<String>,
    pub expected_result: String,
    pub deadline: Option<NaiveDate>,
    pub deadline_raw: Option<String>,
    pub original_message: String,
    pub message_type: MessageType,
    pub ai_model_used: String,
    pub ai_response_raw: String,
    pub status: TaskStatus,
    pub priority: TaskPriority,
    pub blocked_reason: Option<String>,
    pub telegram_chat_id: i64,
    pub telegram_message_id: i32,
    pub telegram_task_message_id: Option<i32>,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
    pub started_at: Option<DateTime<Utc>>,
    pub blocked_at: Option<DateTime<Utc>>,
    pub review_requested_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub cancelled_at: Option<DateTime<Utc>>,
    pub updated_at: DateTime<Utc>,
}

/// Aggregate statistics computed over a set of tasks for a single user.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    pub created_count: i64,
    pub completed_count: i64,
    pub active_count: i64,
    pub overdue_count: i64,
    pub average_completion_hours: Option<i64>,
}

impl Task {
    /// Creates a new task from an AI-produced `StructuredTaskDraft`.
    ///
    /// Validates all draft business rules before constructing the entity so that
    /// invalid drafts are rejected at the domain boundary rather than persisted.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source_message_key: String,
        created_by_user_id: i64,
        assigned_to_user_id: Option<i64>,
        assigned_to_employee_id: Option<i64>,
        draft: StructuredTaskDraft,
        deadline: Option<NaiveDate>,
        deadline_raw: Option<String>,
        original_message: String,
        message_type: MessageType,
        ai_model_used: String,
        ai_response_raw: String,
        telegram_chat_id: i64,
        telegram_message_id: i32,
        now: DateTime<Utc>,
    ) -> AppResult<Self> {
        draft.validate_business_rules()?;

        Ok(Self {
            id: None,
            task_uid: Uuid::now_v7(),
            version: 0,
            source_message_key,
            created_by_user_id,
            assigned_to_user_id,
            assigned_to_employee_id,
            title: draft.title,
            description: draft.steps.join("\n"),
            acceptance_criteria: draft.acceptance_criteria,
            expected_result: draft.expected_result,
            deadline,
            deadline_raw,
            original_message,
            message_type,
            ai_model_used,
            ai_response_raw,
            status: TaskStatus::Created,
            priority: TaskPriority::Medium,
            blocked_reason: None,
            telegram_chat_id,
            telegram_message_id,
            telegram_task_message_id: None,
            tags: Vec::new(),
            created_at: now,
            sent_at: None,
            started_at: None,
            blocked_at: None,
            review_requested_at: None,
            completed_at: None,
            cancelled_at: None,
            updated_at: now,
        })
    }

    /// Advances the task to `next_status`, recording the transition timestamp.
    ///
    /// Returns an error if the transition is not permitted by the state machine.
    pub fn transition_to(&self, next_status: TaskStatus, now: DateTime<Utc>) -> AppResult<Self> {
        if !self.status.can_transition_to(next_status) {
            return Err(AppError::business_rule(
                "TASK_TRANSITION_INVALID",
                "Task status transition is not allowed",
                json!({
                    "from": self.status,
                    "to": next_status,
                    "task_uid": self.task_uid,
                }),
            ));
        }

        let mut next_task = self.clone();
        next_task.version += 1;
        next_task.status = next_status;
        next_task.updated_at = now;
        next_task.sent_at = mark_timestamp(next_status, TaskStatus::Sent, self.sent_at, now);
        next_task.started_at =
            mark_timestamp(next_status, TaskStatus::InProgress, self.started_at, now);
        next_task.blocked_at =
            mark_timestamp(next_status, TaskStatus::Blocked, self.blocked_at, now);
        next_task.review_requested_at = mark_timestamp(
            next_status,
            TaskStatus::InReview,
            self.review_requested_at,
            now,
        );
        next_task.completed_at =
            mark_timestamp(next_status, TaskStatus::Completed, self.completed_at, now);
        next_task.cancelled_at =
            mark_timestamp(next_status, TaskStatus::Cancelled, self.cancelled_at, now);
        if next_status != TaskStatus::Blocked {
            next_task.blocked_reason = None;
        }
        Ok(next_task)
    }

    /// Transitions the task to `Blocked` and records the human-readable blocker reason.
    ///
    /// Blockers are stored on the task so the current blocking reason is visible in the
    /// card without forcing the UI to reconstruct it from the full comment history.
    pub fn apply_blocker(&self, reason: impl Into<String>, now: DateTime<Utc>) -> AppResult<Self> {
        let normalized_reason = reason.into().trim().to_owned();
        if normalized_reason.is_empty() {
            return Err(AppError::business_rule(
                "TASK_BLOCKER_EMPTY",
                "Blocker reason cannot be empty",
                json!({}),
            ));
        }

        if normalized_reason.chars().count() > MAX_TASK_BLOCKER_REASON_LENGTH {
            return Err(AppError::business_rule(
                "TASK_BLOCKER_TOO_LONG",
                "Blocker reason is too long",
                json!({ "limit": MAX_TASK_BLOCKER_REASON_LENGTH }),
            ));
        }

        let mut next_task = self.transition_to(TaskStatus::Blocked, now)?;
        next_task.blocked_reason = Some(normalized_reason);
        Ok(next_task)
    }

    /// Assigns the task to a new owner and resets all progress timestamps.
    ///
    /// Reassignment resets ownership-specific progress to avoid leaving a new
    /// assignee with a stale in-progress/review state that belongs to the previous executor.
    pub fn reassign(
        &self,
        assigned_to_user_id: Option<i64>,
        assigned_to_employee_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> AppResult<Self> {
        if self.status.is_terminal() {
            return Err(AppError::business_rule(
                "TASK_REASSIGN_TERMINAL",
                "Completed or cancelled task cannot be reassigned",
                json!({ "task_uid": self.task_uid, "status": self.status }),
            ));
        }

        let mut next_task = self.clone();
        next_task.version += 1;
        next_task.assigned_to_user_id = assigned_to_user_id;
        next_task.assigned_to_employee_id = assigned_to_employee_id;
        next_task.status = TaskStatus::Created;
        next_task.started_at = None;
        next_task.blocked_at = None;
        next_task.review_requested_at = None;
        next_task.completed_at = None;
        next_task.cancelled_at = None;
        next_task.sent_at = None;
        next_task.blocked_reason = None;
        next_task.updated_at = now;
        Ok(next_task)
    }

    /// Links a concrete user account to a task that was previously assigned only via
    /// the employee directory (i.e. before the assignee had registered).
    ///
    /// Late registration links an already-assigned employee directory record to a
    /// concrete user account without resetting the current business status of the task.
    pub fn link_registered_assignee(
        &self,
        assigned_to_user_id: i64,
        now: DateTime<Utc>,
    ) -> AppResult<Self> {
        if self.assigned_to_employee_id.is_none() {
            return Err(AppError::business_rule(
                "TASK_EMPLOYEE_ASSIGNMENT_MISSING",
                "Task cannot be linked to a registered assignee without an employee assignment",
                json!({ "task_uid": self.task_uid }),
            ));
        }

        if self.assigned_to_user_id == Some(assigned_to_user_id) {
            return Ok(self.clone());
        }

        let mut next_task = self.clone();
        next_task.version += 1;
        next_task.assigned_to_user_id = Some(assigned_to_user_id);
        next_task.updated_at = now;
        Ok(next_task)
    }

    /// Returns `true` when the task has a different person as creator and assignee,
    /// meaning the assignee must explicitly accept/review the work before it closes.
    pub fn review_required(&self) -> bool {
        matches!(self.assigned_to_user_id, Some(user_id) if user_id != self.created_by_user_id)
    }

    /// Renders the task as a Telegram-ready text message body.
    ///
    /// Includes an optional mention of the assignee at the top of the card.
    pub fn render_for_telegram(&self, assignee_mention: Option<&str>) -> String {
        let mut lines = Vec::new();

        if let Some(mention) = assignee_mention {
            lines.push(mention.to_owned());
            lines.push(String::new());
        }

        lines.push(format!("Заголовок: {}", self.title));
        lines.push(String::new());
        lines.push("Описание (пошагово):".to_owned());

        for (index, step) in self.description.lines().enumerate() {
            lines.push(format!("{}. {}", index + 1, step));
        }

        lines.push(String::new());
        lines.push(format!("Ожидаемый результат: {}", self.expected_result));

        if !self.acceptance_criteria.is_empty() {
            lines.push(String::new());
            lines.push("Критерии приёма:".to_owned());
            for criterion in &self.acceptance_criteria {
                lines.push(format!("- {}", criterion));
            }
        }

        lines.push(String::new());
        lines.push(format!(
            "Срок выполнения: {}",
            self.deadline
                .map(|date| date.format("%d.%m.%Y").to_string())
                .unwrap_or_else(|| "Срок не указан".to_owned())
        ));
        lines.join("\n")
    }
}

/// Sets a status-specific timestamp on first entry.
///
/// If `next_status == target_status` and no timestamp has been recorded yet,
/// records `now`. Otherwise preserves the existing value so re-entries (e.g.
/// going Blocked → InProgress → Blocked again) do not overwrite the first record.
fn mark_timestamp(
    next_status: TaskStatus,
    target_status: TaskStatus,
    current_value: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    if next_status == target_status {
        return current_value.or(Some(now));
    }
    current_value
}
