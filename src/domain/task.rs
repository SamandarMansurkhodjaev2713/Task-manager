use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fmt::{Display, Formatter};
use uuid::Uuid;
use validator::Validate;

use crate::domain::errors::{AppError, AppResult};
use crate::shared::constants::limits::{
    MAX_ACCEPTANCE_CRITERIA, MAX_TASK_ACCEPTANCE_CRITERION_LENGTH, MAX_TASK_BLOCKER_REASON_LENGTH,
    MAX_TASK_EXPECTED_RESULT_LENGTH, MAX_TASK_STEPS, MAX_TASK_STEP_LENGTH, MAX_TASK_TITLE_LENGTH,
    MIN_TASK_STEPS,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Created,
    Sent,
    InProgress,
    Blocked,
    InReview,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    Medium,
    High,
    Urgent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageType {
    Text,
    Voice,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct StructuredTaskDraft {
    #[validate(length(min = 1, max = 100))]
    pub title: String,
    #[validate(length(min = 1))]
    pub expected_result: String,
    pub steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStats {
    pub created_count: i64,
    pub completed_count: i64,
    pub active_count: i64,
    pub overdue_count: i64,
    pub average_completion_hours: Option<i64>,
}

impl StructuredTaskDraft {
    pub fn validate_business_rules(&self) -> AppResult<()> {
        self.validate().map_err(|error| {
            AppError::schema_validation(
                "TASK_DRAFT_INVALID",
                "AI response does not match the expected schema",
                json!({ "errors": error.to_string() }),
            )
        })?;

        if !(MIN_TASK_STEPS..=MAX_TASK_STEPS).contains(&self.steps.len()) {
            return Err(AppError::business_rule(
                "TASK_STEPS_INVALID",
                "Task must contain between 1 and 7 concrete steps",
                json!({ "count": self.steps.len() }),
            ));
        }

        if self.acceptance_criteria.len() > MAX_ACCEPTANCE_CRITERIA {
            return Err(AppError::business_rule(
                "TASK_ACCEPTANCE_CRITERIA_INVALID",
                "Task contains too many acceptance criteria",
                json!({ "count": self.acceptance_criteria.len() }),
            ));
        }

        if self.title.chars().count() > MAX_TASK_TITLE_LENGTH {
            return Err(AppError::business_rule(
                "TASK_TITLE_TOO_LONG",
                "Task title is too long",
                json!({ "limit": MAX_TASK_TITLE_LENGTH }),
            ));
        }

        if self.expected_result.chars().count() > MAX_TASK_EXPECTED_RESULT_LENGTH {
            return Err(AppError::business_rule(
                "TASK_EXPECTED_RESULT_TOO_LONG",
                "Task expected result is too long",
                json!({ "limit": MAX_TASK_EXPECTED_RESULT_LENGTH }),
            ));
        }

        if let Some(step_length) = self
            .steps
            .iter()
            .map(|value| value.chars().count())
            .find(|value| *value > MAX_TASK_STEP_LENGTH)
        {
            return Err(AppError::business_rule(
                "TASK_STEP_TOO_LONG",
                "Task contains a step that is too long for Telegram delivery",
                json!({ "limit": MAX_TASK_STEP_LENGTH, "length": step_length }),
            ));
        }

        if let Some(criterion_length) = self
            .acceptance_criteria
            .iter()
            .map(|value| value.chars().count())
            .find(|value| *value > MAX_TASK_ACCEPTANCE_CRITERION_LENGTH)
        {
            return Err(AppError::business_rule(
                "TASK_ACCEPTANCE_CRITERION_TOO_LONG",
                "Task contains an acceptance criterion that is too long for Telegram delivery",
                json!({
                    "limit": MAX_TASK_ACCEPTANCE_CRITERION_LENGTH,
                    "length": criterion_length,
                }),
            ));
        }

        Ok(())
    }
}

impl Task {
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

    /// Blockers are stored on the task so the current blocking reason is visible in the card
    /// without forcing the UI to reconstruct it from the full comment history.
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

    /// Reassignment resets ownership-specific progress to avoid leaving a new assignee with a
    /// stale in-progress/review state that belongs to the previous executor.
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

    /// Late registration links an already assigned employee directory record to a concrete user
    /// account without resetting the current business status of the task.
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

    pub fn review_required(&self) -> bool {
        matches!(self.assigned_to_user_id, Some(user_id) if user_id != self.created_by_user_id)
    }

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

impl TaskStatus {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Created, Self::Sent)
                | (Self::Created, Self::InProgress)
                | (Self::Created, Self::Blocked)
                | (Self::Created, Self::Cancelled)
                | (Self::Sent, Self::InProgress)
                | (Self::Sent, Self::Blocked)
                | (Self::Sent, Self::InReview)
                | (Self::Sent, Self::Cancelled)
                | (Self::InProgress, Self::Blocked)
                | (Self::InProgress, Self::InReview)
                | (Self::InProgress, Self::Cancelled)
                | (Self::Blocked, Self::InProgress)
                | (Self::Blocked, Self::InReview)
                | (Self::Blocked, Self::Cancelled)
                | (Self::InReview, Self::InProgress)
                | (Self::InReview, Self::Completed)
                | (Self::InReview, Self::Cancelled)
        )
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled)
    }
}

impl Display for TaskStatus {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Created => "created",
            Self::Sent => "sent",
            Self::InProgress => "in_progress",
            Self::Blocked => "blocked",
            Self::InReview => "in_review",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
        };
        formatter.write_str(value)
    }
}

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
