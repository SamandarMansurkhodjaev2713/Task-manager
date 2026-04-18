use std::sync::Arc;

use chrono::NaiveDate;
use serde_json::json;
use uuid::Uuid;

use crate::application::dto::task_views::{DeliveryStatus, TaskCommentView, TaskStatusDetails};
use crate::application::policies::role_authorization::RoleAuthorizationPolicy;
use crate::application::ports::repositories::{
    AuditLogRepository, CommentRepository, EmployeeRepository, NotificationRepository,
    TaskRepository, UserRepository,
};
use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::notification::NotificationType;
use crate::domain::task::Task;
use crate::domain::user::User;
use crate::shared::constants::limits::MAX_TASK_CONTEXT_PREVIEW_COMMENTS;
use crate::shared::task_codes::{
    format_public_task_code_or_placeholder, parse_task_reference, TaskReference,
};

pub struct GetTaskStatusUseCase {
    task_repository: Arc<dyn TaskRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
    user_repository: Arc<dyn UserRepository>,
    employee_repository: Arc<dyn EmployeeRepository>,
    comment_repository: Arc<dyn CommentRepository>,
}

impl GetTaskStatusUseCase {
    pub fn new(
        task_repository: Arc<dyn TaskRepository>,
        audit_log_repository: Arc<dyn AuditLogRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
        user_repository: Arc<dyn UserRepository>,
        employee_repository: Arc<dyn EmployeeRepository>,
        comment_repository: Arc<dyn CommentRepository>,
    ) -> Self {
        Self {
            task_repository,
            audit_log_repository,
            notification_repository,
            user_repository,
            employee_repository,
            comment_repository,
        }
    }

    pub async fn execute(&self, actor: &User, task_uid: Uuid) -> AppResult<TaskStatusDetails> {
        let Some(task) = self.task_repository.find_by_uid(task_uid).await? else {
            return Err(AppError::not_found(
                "TASK_NOT_FOUND",
                "Task was not found",
                json!({ "task_uid": task_uid }),
            ));
        };
        RoleAuthorizationPolicy::ensure_can_view_task(actor, &task)?;

        let Some(task_id) = task.id else {
            return Err(AppError::internal(
                "TASK_ID_MISSING",
                "Task must have a database identifier before loading details",
                json!({ "task_uid": task_uid }),
            ));
        };

        let history = self.audit_log_repository.list_for_task(task_id).await?;
        let comments = self
            .comment_repository
            .list_recent_for_task(task_id, MAX_TASK_CONTEXT_PREVIEW_COMMENTS as i64)
            .await?;
        let assignee_display = self.resolve_assignee_display(&task).await?;
        let delivery_status = self.resolve_delivery_status(&task).await?;

        Ok(TaskStatusDetails {
            task_uid,
            public_code: format_public_task_code_or_placeholder(task.id),
            title: task.title.clone(),
            status: task.status,
            deadline: task.deadline.map(format_deadline),
            expected_result: task.expected_result.clone(),
            description_lines: split_description_lines(&task.description),
            acceptance_criteria: task.acceptance_criteria.clone(),
            history_entries: history.iter().map(render_history_entry).collect(),
            assignee_display,
            delivery_status,
            blocked_reason: task.blocked_reason.clone(),
            comments: comments.iter().map(TaskCommentView::from_comment).collect(),
            available_actions: RoleAuthorizationPolicy::available_actions(actor, &task),
        })
    }

    pub async fn resolve_task_uid(&self, reference: &str) -> AppResult<Uuid> {
        let Some(parsed_reference) = parse_task_reference(reference) else {
            return Err(AppError::business_rule(
                "TASK_REFERENCE_INVALID",
                "Task reference must be a public task code like T-0042 or a UUID",
                json!({ "reference": reference }),
            ));
        };

        let task = match parsed_reference {
            TaskReference::PublicId(task_id) => self.task_repository.find_by_id(task_id).await?,
            TaskReference::Uid(task_uid) => self.task_repository.find_by_uid(task_uid).await?,
        };

        task.map(|value| value.task_uid).ok_or_else(|| {
            AppError::not_found(
                "TASK_NOT_FOUND",
                "Task was not found",
                json!({ "reference": reference }),
            )
        })
    }

    async fn resolve_assignee_display(&self, task: &Task) -> AppResult<Option<String>> {
        if let Some(user_id) = task.assigned_to_user_id {
            if let Some(user) = self.user_repository.find_by_id(user_id).await? {
                let display = user
                    .telegram_username
                    .map(|value| format!("@{value}"))
                    .or(user.full_name)
                    .unwrap_or_else(|| format!("user:{user_id}"));
                return Ok(Some(display));
            }
        }

        let Some(employee_id) = task.assigned_to_employee_id else {
            return Ok(None);
        };
        Ok(self
            .employee_repository
            .find_by_id(employee_id)
            .await?
            .map(|employee| employee.full_name))
    }

    async fn resolve_delivery_status(&self, task: &Task) -> AppResult<Option<DeliveryStatus>> {
        let has_assignee =
            task.assigned_to_user_id.is_some() || task.assigned_to_employee_id.is_some();
        if !has_assignee {
            return Ok(Some(DeliveryStatus::CreatorOnly));
        }

        let Some(task_id) = task.id else {
            return Ok(None);
        };

        let Some(user_id) = task.assigned_to_user_id else {
            return Ok(Some(DeliveryStatus::PendingAssigneeRegistration));
        };

        let Some(user) = self.user_repository.find_by_id(user_id).await? else {
            return Ok(Some(DeliveryStatus::PendingAssigneeRegistration));
        };

        let direct_delivery_possible = user.last_chat_id.is_some();
        let latest_assignment = self
            .notification_repository
            .find_latest_for_task_and_recipient(task_id, user_id, NotificationType::TaskAssigned)
            .await?;

        Ok(Some(DeliveryStatus::from_assignment_notification(
            latest_assignment.map(|value| value.delivery_state),
            has_assignee,
            direct_delivery_possible,
        )))
    }
}

fn split_description_lines(description: &str) -> Vec<String> {
    description
        .lines()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn format_deadline(value: NaiveDate) -> String {
    value.format("%d.%m.%Y").to_string()
}

fn render_history_status(value: Option<&str>) -> &'static str {
    match value {
        Some("created") => "новая",
        Some("sent") => "отправлена",
        Some("in_progress") => "в работе",
        Some("blocked") => "с блокером",
        Some("in_review") => "на проверке",
        Some("completed") => "завершена",
        Some("cancelled") => "отменена",
        Some(_) | None => "неизвестно",
    }
}

fn render_history_entry(entry: &AuditLogEntry) -> String {
    let timestamp = entry.created_at.format("%d.%m.%Y %H:%M");
    let details = match entry.action {
        AuditAction::Created => "задача создана".to_owned(),
        AuditAction::Sent => "уведомление доставлено исполнителю".to_owned(),
        AuditAction::Assigned | AuditAction::Reassigned => "исполнитель обновлён".to_owned(),
        AuditAction::ReviewRequested => "задача отправлена на проверку".to_owned(),
        AuditAction::Blocked => entry
            .metadata
            .get("reason")
            .and_then(|value| value.as_str())
            .map(|reason| format!("зафиксирован блокер: {reason}"))
            .unwrap_or_else(|| "зафиксирован блокер".to_owned()),
        AuditAction::Commented => entry
            .metadata
            .get("preview")
            .and_then(|value| value.as_str())
            .map(|preview| format!("добавлен комментарий: {preview}"))
            .unwrap_or_else(|| "добавлен комментарий".to_owned()),
        AuditAction::StatusChanged | AuditAction::Cancelled => format!(
            "{} → {}",
            render_history_status(entry.old_status.as_deref()),
            render_history_status(entry.new_status.as_deref())
        ),
        AuditAction::Edited => "карточка задачи обновлена".to_owned(),
        AuditAction::EmployeesSynced => "справочник сотрудников синхронизирован".to_owned(),
    };

    format!("{timestamp}: {details}")
}
