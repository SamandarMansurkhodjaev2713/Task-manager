use uuid::Uuid;

use crate::application::dto::task_views::TaskActionView;
use crate::domain::task::TaskStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskListOrigin {
    Assigned,
    Created,
    Team,
    Focus,
    ManagerInbox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCardMode {
    Compact,
    Expanded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftEditField {
    Assignee,
    Description,
    Deadline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TelegramCallback {
    MenuHome,
    MenuHelp,
    MenuSettings,
    MenuStats,
    MenuTeamStats,
    MenuCreate,
    MenuSyncEmployees,
    ListTasks {
        origin: TaskListOrigin,
        cursor: Option<String>,
    },
    OpenTask {
        task_uid: Uuid,
        origin: TaskListOrigin,
        mode: TaskCardMode,
    },
    UpdateTaskStatus {
        task_uid: Uuid,
        next_status: TaskStatus,
        origin: TaskListOrigin,
    },
    ConfirmTaskCancel {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    ExecuteTaskCancel {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    StartTaskCommentInput {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    StartTaskBlockerInput {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    StartTaskReassignInput {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    ShowDeliveryHelp {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    StartQuickCreate,
    StartGuidedCreate,
    VoiceCreateConfirm,
    VoiceCreateEdit,
    VoiceCreateBack,
    VoiceCreateCancel,
    DraftSkipAssignee,
    DraftSkipDeadline,
    DraftSubmit,
    DraftEdit {
        field: DraftEditField,
    },
}

pub fn action_to_status(action: TaskActionView) -> Option<TaskStatus> {
    match action {
        TaskActionView::StartProgress => Some(TaskStatus::InProgress),
        TaskActionView::SubmitForReview => Some(TaskStatus::InReview),
        TaskActionView::ApproveReview => Some(TaskStatus::Completed),
        TaskActionView::ReturnToWork => Some(TaskStatus::InProgress),
        TaskActionView::Cancel
        | TaskActionView::ReportBlocker
        | TaskActionView::AddComment
        | TaskActionView::Reassign => None,
    }
}

impl TelegramCallback {
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Self::UpdateTaskStatus { .. }
                | Self::ExecuteTaskCancel { .. }
                | Self::DraftSubmit
                | Self::VoiceCreateConfirm
        )
    }
}
