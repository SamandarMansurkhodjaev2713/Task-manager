use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

/// The lifecycle status of a task.
///
/// Valid transitions are encoded in [`TaskStatus::can_transition_to`].
/// Terminal states (`Completed`, `Cancelled`) cannot transition further.
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

impl TaskStatus {
    /// Returns `true` if moving from `self` to `next` is a permitted state-machine edge.
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

    /// Returns `true` for statuses that no further work can reverse (`Completed` / `Cancelled`).
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

#[cfg(test)]
mod tests {
    use super::TaskStatus;

    #[test]
    fn created_can_transition_to_sent() {
        assert!(TaskStatus::Created.can_transition_to(TaskStatus::Sent));
    }

    #[test]
    fn completed_cannot_transition_to_any_status() {
        for next in [
            TaskStatus::Created,
            TaskStatus::Sent,
            TaskStatus::InProgress,
            TaskStatus::Blocked,
            TaskStatus::InReview,
            TaskStatus::Completed,
            TaskStatus::Cancelled,
        ] {
            assert!(
                !TaskStatus::Completed.can_transition_to(next),
                "Completed must not transition to {next:?}"
            );
        }
    }

    #[test]
    fn cancelled_is_terminal() {
        assert!(TaskStatus::Cancelled.is_terminal());
    }

    #[test]
    fn in_progress_is_not_terminal() {
        assert!(!TaskStatus::InProgress.is_terminal());
    }
}
