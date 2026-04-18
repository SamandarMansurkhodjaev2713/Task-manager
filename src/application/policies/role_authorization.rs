use serde_json::json;

use crate::application::dto::task_views::TaskActionView;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::task::{Task, TaskStatus};
use crate::domain::user::User;

pub struct RoleAuthorizationPolicy;

impl RoleAuthorizationPolicy {
    pub fn ensure_can_view_team_tasks(actor: &User) -> AppResult<()> {
        if actor.role.is_manager_or_admin() {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "Only managers and admins can view team task dashboards",
            json!({ "telegram_id": actor.telegram_id }),
        ))
    }

    pub fn ensure_can_view_team_stats(actor: &User) -> AppResult<()> {
        if actor.role.is_manager_or_admin() {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "Only managers and admins can view team stats",
            json!({ "telegram_id": actor.telegram_id }),
        ))
    }

    pub fn ensure_can_sync_employees(actor: &User) -> AppResult<()> {
        if actor.role.is_admin() {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "Only admins can trigger employee sync",
            json!({ "telegram_id": actor.telegram_id }),
        ))
    }

    pub fn ensure_can_view_task(actor: &User, task: &Task) -> AppResult<()> {
        let actor_id = required_actor_id(actor, "view task status")?;

        if actor.role.is_manager_or_admin()
            || actor_id == task.created_by_user_id
            || task.assigned_to_user_id == Some(actor_id)
        {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "User is not allowed to view this task",
            json!({ "task_uid": task.task_uid }),
        ))
    }

    pub fn ensure_can_comment(actor: &User, task: &Task) -> AppResult<()> {
        let actor_id = required_actor_id(actor, "comment on a task")?;

        if actor.role.is_manager_or_admin()
            || actor_id == task.created_by_user_id
            || task.assigned_to_user_id == Some(actor_id)
        {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "User is not allowed to comment on this task",
            json!({ "task_uid": task.task_uid }),
        ))
    }

    pub fn ensure_can_report_blocker(actor: &User, task: &Task) -> AppResult<()> {
        let actor_id = required_actor_id(actor, "report a blocker")?;

        if actor.role.is_admin() || task.assigned_to_user_id == Some(actor_id) {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "Only the assignee or admin can mark a task as blocked",
            json!({ "task_uid": task.task_uid }),
        ))
    }

    pub fn ensure_can_reassign(actor: &User, task: &Task) -> AppResult<()> {
        let actor_id = required_actor_id(actor, "reassign tasks")?;
        let can_reassign = actor.role.is_manager_or_admin()
            || task.created_by_user_id == actor_id
            || task.assigned_to_user_id == Some(actor_id);
        if can_reassign {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "Only the creator, assignee, manager, or admin can reassign a task",
            json!({
                "actor_user_id": actor_id,
                "task_uid": task.task_uid,
            }),
        ))
    }

    pub fn normalize_requested_status(
        actor: &User,
        task: &Task,
        requested_status: TaskStatus,
    ) -> AppResult<TaskStatus> {
        if requested_status == TaskStatus::Completed && task.review_required() {
            let is_creator = actor.id == Some(task.created_by_user_id);
            if !is_creator && !actor.role.is_manager_or_admin() {
                return Ok(TaskStatus::InReview);
            }
        }

        if requested_status == TaskStatus::Completed
            && task.review_required()
            && task.status != TaskStatus::InReview
        {
            return Err(AppError::business_rule(
                "TASK_REVIEW_REQUIRED",
                "Task must go through review before final completion",
                json!({ "task_uid": task.task_uid }),
            ));
        }

        Ok(requested_status)
    }

    pub fn ensure_can_change_status(
        actor: &User,
        task: &Task,
        next_status: TaskStatus,
    ) -> AppResult<()> {
        let actor_id = required_actor_id(actor, "change task status")?;

        if actor.role.is_admin() {
            return Ok(());
        }

        if actor_id == task.created_by_user_id && creator_can_change_status(task, next_status) {
            return Ok(());
        }

        if task.assigned_to_user_id == Some(actor_id) && assignee_can_change_status(next_status) {
            return Ok(());
        }

        if actor.role.is_manager_or_admin() && manager_can_change_status(next_status) {
            return Ok(());
        }

        Err(AppError::unauthorized(
            "User is not allowed to change this task status",
            json!({ "task_uid": task.task_uid, "next_status": next_status }),
        ))
    }

    pub fn available_actions(actor: &User, task: &Task) -> Vec<TaskActionView> {
        let mut actions = Vec::new();
        let actor_id = actor.id;
        let is_assignee = actor_id.is_some() && task.assigned_to_user_id == actor_id;
        let is_creator = actor_id == Some(task.created_by_user_id);
        let is_admin = actor.role.is_admin();
        let can_manage = is_creator || actor.role.is_manager_or_admin();

        if matches!(
            task.status,
            TaskStatus::Created | TaskStatus::Sent | TaskStatus::Blocked
        ) && (is_assignee || is_admin)
        {
            actions.push(TaskActionView::StartProgress);
        }

        if matches!(
            task.status,
            TaskStatus::Sent | TaskStatus::InProgress | TaskStatus::Blocked
        ) && (is_assignee || is_admin)
        {
            actions.push(TaskActionView::SubmitForReview);
        }

        if task.status == TaskStatus::InReview && can_manage {
            actions.push(TaskActionView::ApproveReview);
            actions.push(TaskActionView::ReturnToWork);
        }

        if matches!(
            task.status,
            TaskStatus::Created | TaskStatus::Sent | TaskStatus::InProgress | TaskStatus::Blocked
        ) && (is_assignee || is_admin)
        {
            actions.push(TaskActionView::ReportBlocker);
        }

        if !task.status.is_terminal() && can_manage {
            actions.push(TaskActionView::Reassign);
        }

        // `can_manage` already includes `is_manager_or_admin()` which covers admins;
        // the separate `|| is_admin` was redundant dead code.
        if !task.status.is_terminal() && (is_assignee || can_manage) {
            actions.push(TaskActionView::Cancel);
        }

        actions.push(TaskActionView::AddComment);
        actions
    }
}

fn required_actor_id(actor: &User, action: &str) -> AppResult<i64> {
    actor.id.ok_or_else(|| {
        AppError::unauthenticated(
            format!("User must be registered before they can {action}"),
            json!({ "telegram_id": actor.telegram_id }),
        )
    })
}

fn creator_can_change_status(task: &Task, next_status: TaskStatus) -> bool {
    match next_status {
        TaskStatus::Completed | TaskStatus::Cancelled => true,
        TaskStatus::InProgress => task.status == TaskStatus::InReview,
        TaskStatus::Created | TaskStatus::Sent | TaskStatus::Blocked | TaskStatus::InReview => {
            false
        }
    }
}

fn assignee_can_change_status(next_status: TaskStatus) -> bool {
    matches!(
        next_status,
        TaskStatus::InProgress | TaskStatus::InReview | TaskStatus::Blocked | TaskStatus::Cancelled
    )
}

fn manager_can_change_status(next_status: TaskStatus) -> bool {
    matches!(
        next_status,
        TaskStatus::InProgress | TaskStatus::Completed | TaskStatus::Cancelled
    )
}

#[cfg(test)]
mod tests {
    use super::RoleAuthorizationPolicy;
    use crate::domain::task::{MessageType, Task, TaskPriority, TaskStatus};
    use crate::domain::user::{User, UserRole};
    use uuid::Uuid;

    #[test]
    fn given_employee_when_viewing_unrelated_team_task_then_policy_rejects_access() {
        let actor = user(Some(7), UserRole::User);
        let task = task(11, Some(22), TaskStatus::Created);

        let result = RoleAuthorizationPolicy::ensure_can_view_task(&actor, &task);

        assert!(result.is_err());
    }

    #[test]
    fn given_manager_when_viewing_team_task_then_policy_allows_access() {
        let actor = user(Some(7), UserRole::Manager);
        let task = task(11, Some(22), TaskStatus::Created);

        let result = RoleAuthorizationPolicy::ensure_can_view_task(&actor, &task);

        assert!(result.is_ok());
    }

    #[test]
    fn given_assignee_when_completing_review_required_task_then_policy_redirects_to_review() {
        let actor = user(Some(22), UserRole::User);
        let mut task = task(11, Some(22), TaskStatus::InProgress);
        task.expected_result = "Ready for review".to_owned();

        let normalized = RoleAuthorizationPolicy::normalize_requested_status(
            &actor,
            &task,
            TaskStatus::Completed,
        )
        .expect("normalization should succeed");

        assert_eq!(normalized, TaskStatus::InReview);
    }

    fn user(id: Option<i64>, role: UserRole) -> User {
        User {
            id,
            telegram_id: 100,
            last_chat_id: Some(100),
            telegram_username: Some("tester".to_owned()),
            full_name: Some("Tester".to_owned()),
            linked_employee_id: Some(5),
            is_employee: true,
            role,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    fn task(created_by_user_id: i64, assigned_to_user_id: Option<i64>, status: TaskStatus) -> Task {
        Task {
            id: Some(1),
            task_uid: Uuid::now_v7(),
            version: 0,
            source_message_key: "telegram:1:1".to_owned(),
            created_by_user_id,
            assigned_to_user_id,
            assigned_to_employee_id: Some(5),
            title: "Prepare release".to_owned(),
            description: "1. Prepare release".to_owned(),
            acceptance_criteria: vec!["Release checklist done".to_owned()],
            expected_result: "Release is ready".to_owned(),
            deadline: None,
            deadline_raw: None,
            original_message: "prepare release".to_owned(),
            message_type: MessageType::Text,
            ai_model_used: "test".to_owned(),
            ai_response_raw: "{}".to_owned(),
            status,
            priority: TaskPriority::Medium,
            blocked_reason: None,
            telegram_chat_id: 1,
            telegram_message_id: 1,
            telegram_task_message_id: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            sent_at: None,
            started_at: None,
            blocked_at: None,
            review_requested_at: None,
            completed_at: None,
            cancelled_at: None,
            updated_at: chrono::Utc::now(),
        }
    }
}
