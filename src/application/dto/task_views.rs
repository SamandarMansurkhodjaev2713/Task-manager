use chrono::NaiveDate;
use serde::Serialize;
use uuid::Uuid;

use crate::domain::comment::{CommentKind, TaskComment};
use crate::domain::employee::EmployeeMatch;
use crate::domain::notification::NotificationDeliveryState;
use crate::domain::task::{Task, TaskStats, TaskStatus};
use crate::shared::constants::limits::MAX_CLARIFICATION_TASK_BODY_PREVIEW_CHARS;
use crate::shared::task_codes::format_public_task_code_or_placeholder;

#[derive(Debug, Clone, Serialize)]
pub enum TaskCreationOutcome {
    Created(TaskCreationSummary),
    DuplicateFound(TaskCreationSummary),
    ClarificationRequired(ClarificationRequest),
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskInterpretationPreview {
    pub description: String,
    pub deadline_label: Option<String>,
    pub assignee: AssigneeInterpretation,
}

#[derive(Debug, Clone, Serialize)]
pub enum AssigneeInterpretation {
    None,
    Resolved {
        display: String,
        delivery_status: DeliveryStatus,
    },
    ClarificationRequired(ClarificationRequest),
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskCreationSummary {
    pub task_uid: Uuid,
    pub public_code: String,
    pub message: String,
    pub delivery_status: DeliveryStatus,
    pub task: TaskListItem,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClarificationRequest {
    pub message: String,
    pub requested_query: Option<String>,
    pub allow_unassigned: bool,
    pub candidates: Vec<EmployeeCandidateView>,
    /// Parsed task description (or title when reassigning) so the clarification
    /// screen can repeat it: after many follow-up messages the original Telegram
    /// message scrolls up and is easy to lose.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_body_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmployeeCandidateView {
    pub employee_id: Option<i64>,
    pub full_name: String,
    pub telegram_username: Option<String>,
    pub confidence: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    DeliveredToAssignee,
    PendingDelivery,
    PendingAssigneeRegistration,
    RetryPending,
    Failed,
    CreatorOnly,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskListPage {
    pub sections: Vec<TaskListSection>,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskListSection {
    pub title: String,
    pub tasks: Vec<TaskListItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskListItem {
    pub task_uid: Uuid,
    pub public_code: String,
    pub title: String,
    pub status: TaskStatus,
    pub deadline: Option<NaiveDate>,
    pub assigned_to_display: Option<String>,
    pub delivery_status: Option<DeliveryStatus>,
    pub highlight: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskStatusSummary {
    pub task_uid: Uuid,
    pub public_code: String,
    pub status: TaskStatus,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskCommentView {
    pub kind: CommentKind,
    pub body: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskStatusDetails {
    pub task_uid: Uuid,
    pub public_code: String,
    pub title: String,
    pub status: TaskStatus,
    pub deadline: Option<String>,
    pub expected_result: String,
    pub description_lines: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub history_entries: Vec<String>,
    pub assignee_display: Option<String>,
    pub delivery_status: Option<DeliveryStatus>,
    pub blocked_reason: Option<String>,
    pub comments: Vec<TaskCommentView>,
    pub available_actions: Vec<TaskActionView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsView {
    pub created_count: i64,
    pub completed_count: i64,
    pub active_count: i64,
    pub overdue_count: i64,
    pub average_completion_hours: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskActionView {
    StartProgress,
    SubmitForReview,
    ApproveReview,
    ReturnToWork,
    Cancel,
    ReportBlocker,
    AddComment,
    Reassign,
}

impl TaskCreationSummary {
    pub fn from_task(task: &Task, message: String, delivery_status: DeliveryStatus) -> Self {
        Self {
            task_uid: task.task_uid,
            public_code: format_public_task_code_or_placeholder(task.id),
            message,
            delivery_status,
            task: TaskListItem::from_task(task, None, Some(delivery_status), None),
        }
    }
}

impl EmployeeCandidateView {
    pub fn from_match(value: &EmployeeMatch) -> Self {
        Self {
            employee_id: value.employee.id,
            full_name: value.employee.full_name.clone(),
            telegram_username: value.employee.telegram_username.clone(),
            confidence: value.confidence,
        }
    }
}

impl TaskListItem {
    pub fn from_task(
        task: &Task,
        assigned_to_display: Option<String>,
        delivery_status: Option<DeliveryStatus>,
        highlight: Option<String>,
    ) -> Self {
        Self {
            task_uid: task.task_uid,
            public_code: format_public_task_code_or_placeholder(task.id),
            title: task.title.clone(),
            status: task.status,
            deadline: task.deadline,
            assigned_to_display,
            delivery_status,
            highlight,
        }
    }
}

impl TaskCommentView {
    pub fn from_comment(comment: &TaskComment) -> Self {
        Self {
            kind: comment.kind,
            body: comment.body.clone(),
            created_at: comment.created_at.format("%d.%m.%Y %H:%M").to_string(),
        }
    }
}

impl DeliveryStatus {
    pub fn from_assignment_notification(
        state: Option<NotificationDeliveryState>,
        has_assignee: bool,
        direct_delivery_possible: bool,
    ) -> Self {
        if !has_assignee {
            return Self::CreatorOnly;
        }

        if !direct_delivery_possible {
            return Self::PendingAssigneeRegistration;
        }

        match state {
            Some(NotificationDeliveryState::Sent) => Self::DeliveredToAssignee,
            Some(NotificationDeliveryState::RetryPending) => Self::RetryPending,
            Some(NotificationDeliveryState::Failed) => Self::Failed,
            Some(NotificationDeliveryState::Pending) | None => Self::PendingDelivery,
        }
    }
}

impl From<TaskStats> for StatsView {
    fn from(value: TaskStats) -> Self {
        Self {
            created_count: value.created_count,
            completed_count: value.completed_count,
            active_count: value.active_count,
            overdue_count: value.overdue_count,
            average_completion_hours: value.average_completion_hours,
        }
    }
}

/// Bounded preview of the task text for assignee-clarification UIs.
pub fn format_task_body_preview_for_clarification(
    task_description: &str,
    fallback_full_text: &str,
) -> String {
    let body = task_description.trim();
    let source = if body.is_empty() {
        fallback_full_text.trim()
    } else {
        body
    };
    let limit = MAX_CLARIFICATION_TASK_BODY_PREVIEW_CHARS;
    let n = source.chars().count();
    if n <= limit {
        return source.to_owned();
    }
    format!("{}…", source.chars().take(limit).collect::<String>())
}
