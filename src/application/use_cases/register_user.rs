use std::sync::Arc;

use serde_json::json;

use crate::application::ports::repositories::{
    AuditLogRepository, EmployeeRepository, NotificationRepository, TaskRepository, UserRepository,
};
use crate::application::ports::services::Clock;
use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::employee::Employee;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::message::IncomingMessage;
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::task::Task;
use crate::domain::user::{User, UserRole};
use crate::shared::constants::limits::{
    MAX_REGISTRATION_RECOVERY_BATCHES, REGISTRATION_RECOVERY_TASK_BATCH_SIZE,
};

const TASK_VERSION_CONFLICT_ERROR: &str = "TASK_VERSION_CONFLICT";

pub struct RegisterUserUseCase {
    clock: Arc<dyn Clock>,
    user_repository: Arc<dyn UserRepository>,
    employee_repository: Arc<dyn EmployeeRepository>,
    task_repository: Arc<dyn TaskRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
}

impl RegisterUserUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        clock: Arc<dyn Clock>,
        user_repository: Arc<dyn UserRepository>,
        employee_repository: Arc<dyn EmployeeRepository>,
        task_repository: Arc<dyn TaskRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
        audit_log_repository: Arc<dyn AuditLogRepository>,
    ) -> Self {
        Self {
            clock,
            user_repository,
            employee_repository,
            task_repository,
            notification_repository,
            audit_log_repository,
        }
    }

    pub async fn execute(&self, message: &IncomingMessage) -> AppResult<User> {
        let employees = self.employee_repository.list_active().await?;
        let matched_employee = resolve_registered_employee(message, &employees);
        let user = self
            .persist_user(message, matched_employee.is_some())
            .await?;

        self.recover_pending_assignments(&user, matched_employee.as_ref())
            .await?;
        Ok(user)
    }

    async fn persist_user(&self, message: &IncomingMessage, is_employee: bool) -> AppResult<User> {
        let user = User::from_message(message, UserRole::User, is_employee);
        self.user_repository.upsert_from_message(&user).await
    }

    async fn recover_pending_assignments(
        &self,
        user: &User,
        employee: Option<&Employee>,
    ) -> AppResult<()> {
        let Some(employee_id) = employee.and_then(|value| value.id) else {
            return Ok(());
        };
        let user_id = required_user_id(user)?;

        for _ in 0..MAX_REGISTRATION_RECOVERY_BATCHES {
            let tasks = self
                .task_repository
                .list_open_assigned_to_employee_without_user(
                    employee_id,
                    REGISTRATION_RECOVERY_TASK_BATCH_SIZE,
                )
                .await?;
            if tasks.is_empty() {
                return Ok(());
            }

            let recovered_count = self
                .recover_task_batch(&tasks, user_id, employee_id)
                .await?;
            if recovered_count == 0 || tasks.len() < REGISTRATION_RECOVERY_TASK_BATCH_SIZE as usize
            {
                return Ok(());
            }
        }

        Ok(())
    }

    async fn recover_task_batch(
        &self,
        tasks: &[Task],
        user_id: i64,
        employee_id: i64,
    ) -> AppResult<usize> {
        let mut recovered_count = 0_usize;

        for task in tasks {
            let Some(saved_task) = self
                .link_task_to_registered_user(task, user_id, employee_id)
                .await?
            else {
                continue;
            };

            self.log_recovered_assignment(&saved_task, user_id, employee_id)
                .await?;
            self.ensure_assignment_notification(&saved_task, user_id)
                .await?;
            recovered_count += 1;
        }

        Ok(recovered_count)
    }

    async fn link_task_to_registered_user(
        &self,
        task: &Task,
        user_id: i64,
        employee_id: i64,
    ) -> AppResult<Option<Task>> {
        let now = self.clock.now_utc();
        let updated_task = task.link_registered_assignee(user_id, now)?;

        match self.task_repository.update(&updated_task).await {
            Ok(task) => Ok(Some(task)),
            Err(error) if error.code() == TASK_VERSION_CONFLICT_ERROR => {
                self.retry_link_after_conflict(task, user_id, employee_id)
                    .await
            }
            Err(error) => Err(error),
        }
    }

    async fn retry_link_after_conflict(
        &self,
        task: &Task,
        user_id: i64,
        employee_id: i64,
    ) -> AppResult<Option<Task>> {
        let Some(latest_task) = self.task_repository.find_by_uid(task.task_uid).await? else {
            return Ok(None);
        };
        if !should_link_task(&latest_task, employee_id) {
            return Ok(None);
        }

        let retried_task = latest_task.link_registered_assignee(user_id, self.clock.now_utc())?;
        match self.task_repository.update(&retried_task).await {
            Ok(task) => Ok(Some(task)),
            Err(error) if error.code() == TASK_VERSION_CONFLICT_ERROR => Ok(None),
            Err(error) => Err(error),
        }
    }

    async fn log_recovered_assignment(
        &self,
        task: &Task,
        user_id: i64,
        employee_id: i64,
    ) -> AppResult<()> {
        let Some(task_id) = task.id else {
            return Err(AppError::internal(
                "TASK_ID_MISSING",
                "Recovered task must contain a database identifier",
                json!({ "task_uid": task.task_uid }),
            ));
        };

        let entry = AuditLogEntry {
            id: None,
            task_id,
            action: AuditAction::Assigned,
            old_status: Some(task.status.to_string()),
            new_status: Some(task.status.to_string()),
            changed_by_user_id: Some(user_id),
            metadata: json!({
                "resolution": "assignee_registered",
                "assigned_to_user_id": user_id,
                "assigned_to_employee_id": employee_id,
            }),
            created_at: self.clock.now_utc(),
        };
        let _ = self.audit_log_repository.append(&entry).await?;
        Ok(())
    }

    async fn ensure_assignment_notification(&self, task: &Task, user_id: i64) -> AppResult<()> {
        let Some(task_id) = task.id else {
            return Err(AppError::internal(
                "TASK_ID_MISSING",
                "Task must contain an identifier before scheduling delivery",
                json!({ "task_uid": task.task_uid }),
            ));
        };

        let existing_notification = self
            .notification_repository
            .find_latest_for_task_and_recipient(task_id, user_id, NotificationType::TaskAssigned)
            .await?;
        match existing_notification {
            Some(notification) if should_keep_existing_notification(&notification) => Ok(()),
            Some(notification) => self.requeue_assignment_notification(&notification).await,
            None => self.enqueue_assignment_notification(task, user_id).await,
        }
    }

    async fn requeue_assignment_notification(&self, notification: &Notification) -> AppResult<()> {
        let Some(notification_id) = notification.id else {
            return Ok(());
        };
        self.notification_repository.requeue(notification_id).await
    }

    async fn enqueue_assignment_notification(&self, task: &Task, user_id: i64) -> AppResult<()> {
        let notification = Notification {
            id: None,
            task_id: task.id,
            recipient_user_id: user_id,
            notification_type: NotificationType::TaskAssigned,
            message: task.render_for_telegram(None),
            dedupe_key: assignment_notification_dedupe_key(task, user_id),
            telegram_message_id: None,
            delivery_state: NotificationDeliveryState::Pending,
            is_sent: false,
            is_read: false,
            attempt_count: 0,
            sent_at: None,
            read_at: None,
            next_attempt_at: None,
            last_error_code: None,
            created_at: self.clock.now_utc(),
        };
        let _ = self.notification_repository.enqueue(&notification).await?;
        Ok(())
    }
}

fn resolve_registered_employee(
    message: &IncomingMessage,
    employees: &[Employee],
) -> Option<Employee> {
    exact_username_match(message, employees).or_else(|| unique_full_name_match(message, employees))
}

fn exact_username_match(message: &IncomingMessage, employees: &[Employee]) -> Option<Employee> {
    let username = message.sender_username.as_deref()?.trim_start_matches('@');

    employees.iter().find_map(|employee| {
        employee
            .telegram_username
            .as_deref()
            .filter(|value| value.trim_start_matches('@').eq_ignore_ascii_case(username))
            .map(|_| employee.clone())
    })
}

fn unique_full_name_match(message: &IncomingMessage, employees: &[Employee]) -> Option<Employee> {
    let normalized_name = normalize_person_name(&message.sender_name);
    if normalized_name.is_empty() {
        return None;
    }

    let mut matches = employees
        .iter()
        .filter(|employee| normalize_person_name(&employee.full_name) == normalized_name)
        .cloned();
    let first_match = matches.next()?;
    if matches.next().is_some() {
        return None;
    }

    Some(first_match)
}

fn normalize_person_name(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn required_user_id(user: &User) -> AppResult<i64> {
    user.id.ok_or_else(|| {
        AppError::internal(
            "USER_ID_MISSING",
            "Persisted user must contain a database identifier",
            json!({ "telegram_id": user.telegram_id }),
        )
    })
}

fn should_link_task(task: &Task, employee_id: i64) -> bool {
    task.assigned_to_user_id.is_none()
        && task.assigned_to_employee_id == Some(employee_id)
        && !task.status.is_terminal()
}

fn should_keep_existing_notification(notification: &Notification) -> bool {
    matches!(
        notification.delivery_state,
        NotificationDeliveryState::Pending
            | NotificationDeliveryState::RetryPending
            | NotificationDeliveryState::Sent
    )
}

fn assignment_notification_dedupe_key(task: &Task, user_id: i64) -> String {
    format!("task_assigned:{}:{}", task.task_uid, user_id)
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{normalize_person_name, resolve_registered_employee};
    use crate::domain::employee::Employee;
    use crate::domain::message::{IncomingMessage, MessageContent};

    #[test]
    fn given_unique_full_name_when_resolving_registered_employee_then_matches_without_username() {
        let employee = employee("Jean Dupont", None);
        let message = incoming_message("Jean Dupont", None);

        let resolved = resolve_registered_employee(&message, std::slice::from_ref(&employee));

        assert_eq!(
            resolved.map(|value| value.full_name),
            Some(employee.full_name)
        );
    }

    #[test]
    fn given_duplicate_full_name_when_resolving_registered_employee_then_returns_none() {
        let first = employee("Jean Dupont", None);
        let second = employee("Jean Dupont", Some("@other"));
        let message = incoming_message("Jean Dupont", None);

        let resolved = resolve_registered_employee(&message, &[first, second]);

        assert!(resolved.is_none());
    }

    #[test]
    fn given_username_when_resolving_registered_employee_then_prefers_exact_username_match() {
        let employee = employee("Jean Dupont", Some("@jean"));
        let message = incoming_message("Somebody Else", Some("@jean"));

        let resolved = resolve_registered_employee(&message, std::slice::from_ref(&employee));

        assert_eq!(
            resolved.map(|value| value.full_name),
            Some(employee.full_name)
        );
    }

    #[test]
    fn given_person_name_with_extra_spaces_when_normalizing_then_collapses_whitespace() {
        let normalized = normalize_person_name("  Jean   Dupont  ");

        assert_eq!(normalized, "jean dupont");
    }

    fn employee(full_name: &str, username: Option<&str>) -> Employee {
        let now = Utc::now();
        Employee {
            id: Some(1),
            full_name: full_name.to_owned(),
            telegram_username: username.map(ToOwned::to_owned),
            email: None,
            phone: None,
            department: None,
            is_active: true,
            synced_at: Some(now),
            created_at: now,
            updated_at: now,
        }
    }

    fn incoming_message(sender_name: &str, sender_username: Option<&str>) -> IncomingMessage {
        IncomingMessage {
            chat_id: 1,
            message_id: 1,
            sender_id: 10,
            sender_name: sender_name.to_owned(),
            sender_username: sender_username.map(|value| value.trim_start_matches('@').to_owned()),
            content: MessageContent::Text {
                text: "/start".to_owned(),
            },
            timestamp: Utc::now(),
            source_message_key_override: None,
        }
    }
}
