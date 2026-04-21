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
    RegistrationPickEmployee {
        employee_id: i64,
    },
    RegistrationContinueUnlinked,
    ClarificationPickEmployee {
        employee_id: i64,
    },
    ClarificationCreateUnassigned,
    DraftSkipAssignee,
    DraftSkipDeadline,
    DraftSubmit,
    DraftEdit {
        field: DraftEditField,
    },
    /// User confirmed a specific employee during the guided-creation Assignee
    /// step.  Advances the draft to Description with the employee pre-resolved,
    /// bypassing the fuzzy matcher at submit time.
    GuidedAssigneeConfirm {
        employee_id: i64,
    },
    // ── Admin panel (Phase 4) ────────────────────────────────────────────
    AdminMenu,
    /// List the currently active administrators.
    AdminUsers,
    /// Open a user detail card; we intentionally keep the *primary key*
    /// (`user_id`) in the callback instead of Telegram id so we don't leak
    /// admin-only identifiers into the Telegram payload.
    AdminUserDetails {
        user_id: i64,
    },
    /// Request a nonce for a destructive action (role change / deactivate).
    /// The actual mutation is deferred until the nonce is confirmed.
    AdminUserPrepareRoleChange {
        user_id: i64,
        next_role: AdminRoleOption,
    },
    AdminUserPrepareDeactivate {
        user_id: i64,
    },
    AdminUserPrepareReactivate {
        user_id: i64,
    },
    /// Confirm a pending nonce.  The nonce binds (actor, purpose, payload)
    /// so a stale button cannot be replayed against a different user.
    AdminConfirmNonce {
        nonce: String,
    },
    /// Cancel a pending nonce (does NOT need the nonce itself because we
    /// just drop the UI state).
    AdminCancelPending,
    AdminAudit,
    AdminSecurityAudit,
    AdminFeatures,
    AdminToggleFeature {
        flag_key: String,
    },
}

/// The three roles that can be assigned through the admin panel.  Kept as a
/// separate enum from [`UserRole`](crate::domain::user::UserRole) because
/// the callback codec serialises it using short codes (`u`/`m`/`a`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminRoleOption {
    User,
    Manager,
    Admin,
}

impl AdminRoleOption {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::User => "u",
            Self::Manager => "m",
            Self::Admin => "a",
        }
    }

    pub fn from_code(value: &str) -> Option<Self> {
        match value {
            "u" => Some(Self::User),
            "m" => Some(Self::Manager),
            "a" => Some(Self::Admin),
            _ => None,
        }
    }

    pub fn to_user_role(self) -> crate::domain::user::UserRole {
        use crate::domain::user::UserRole;
        match self {
            Self::User => UserRole::User,
            Self::Manager => UserRole::Manager,
            Self::Admin => UserRole::Admin,
        }
    }
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
                | Self::RegistrationPickEmployee { .. }
                | Self::RegistrationContinueUnlinked
                | Self::ClarificationPickEmployee { .. }
                | Self::ClarificationCreateUnassigned
                | Self::GuidedAssigneeConfirm { .. }
                | Self::AdminConfirmNonce { .. }
                | Self::AdminToggleFeature { .. }
        )
    }
}
