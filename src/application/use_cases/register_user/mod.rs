//! User-registration use case.
//!
//! Handles the full `/start` flow:
//!
//! 1. **Employee linking** — attempts to match the registering Telegram user to
//!    a record in the employee directory.  See [`employee_matching`] for the
//!    matching algorithm.
//! 2. **User upsert** — creates or updates the `users` row.
//! 3. **Registration recovery** — if the user is linked to an employee, any
//!    open tasks previously assigned by employee-directory ID (but lacking a
//!    concrete user-account link) are retroactively linked so notifications and
//!    the assignee view work immediately.

mod employee_matching;

use std::sync::Arc;

use serde_json::json;

use crate::application::dto::task_views::EmployeeCandidateView;
use crate::application::ports::repositories::{
    AuditLogRepository, EmployeeRepository, NotificationRepository, TaskRepository, UserRepository,
};
use crate::application::ports::services::Clock;
use crate::application::use_cases::register_user::employee_matching::{
    candidates_from_matches, resolve_registration_match, RegistrationEmployeeMatch,
};
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
const REGISTRATION_AMBIGUOUS_MESSAGE: &str =
    "Нашёл несколько сотрудников, на которых вы похожи. Выберите себя явно, чтобы задачи не ушли не тому человеку.";

// ─── Public types ─────────────────────────────────────────────────────────────

/// The caller's explicit decision on how to link (or not link) the new user to
/// an employee-directory record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistrationLinkDecision {
    /// Let the use case resolve the link automatically (username / full-name match).
    Auto,
    /// Link the user to this specific employee-directory ID.
    EmployeeId(i64),
    /// Register the user without any employee link.
    ContinueUnlinked,
}

/// Result of the `preview_linking` step — either the use case can proceed
/// automatically, or the user must pick from a set of candidates.
#[derive(Debug, Clone)]
pub enum RegistrationLinkPreview {
    Ready(RegistrationLinkDecision),
    ClarificationRequired(RegistrationLinkClarification),
}

/// Data needed to present the user with an employee-selection keyboard.
#[derive(Debug, Clone)]
pub struct RegistrationLinkClarification {
    pub message: String,
    pub candidates: Vec<EmployeeCandidateView>,
    pub allow_continue_unlinked: bool,
}

// ─── Use case ─────────────────────────────────────────────────────────────────

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

    /// Full registration pipeline: preview linking → auto-resolve → persist → recover tasks.
    ///
    /// Returns an error with code `REGISTRATION_LINK_CLARIFICATION_REQUIRED` when
    /// `preview_linking` cannot resolve the match without user input.
    pub async fn execute(&self, message: &IncomingMessage) -> AppResult<User> {
        match self.preview_linking(message).await? {
            RegistrationLinkPreview::Ready(decision) => {
                self.execute_with_link_decision(message, decision).await
            }
            RegistrationLinkPreview::ClarificationRequired(_) => Err(AppError::business_rule(
                "REGISTRATION_LINK_CLARIFICATION_REQUIRED",
                "Registration requires explicit employee clarification",
                json!({ "telegram_id": message.sender_id }),
            )),
        }
    }

    /// Returns the linking outcome that would result from calling [`execute`] with
    /// the same message, without persisting anything.
    ///
    /// Used by the presentation layer to decide whether to show an employee-selection
    /// keyboard before committing the registration.
    pub async fn preview_linking(
        &self,
        message: &IncomingMessage,
    ) -> AppResult<RegistrationLinkPreview> {
        let existing_user = self
            .user_repository
            .find_by_telegram_id(message.sender_id)
            .await?;

        // Already linked — skip expensive matching.
        if let Some(linked_employee_id) = existing_user
            .as_ref()
            .and_then(|user| user.linked_employee_id)
        {
            return Ok(RegistrationLinkPreview::Ready(
                RegistrationLinkDecision::EmployeeId(linked_employee_id),
            ));
        }

        // Existing user who chose "continue unlinked" — only retry on /start.
        if existing_user.is_some() && !is_explicit_start_command(message) {
            return Ok(RegistrationLinkPreview::Ready(
                RegistrationLinkDecision::ContinueUnlinked,
            ));
        }

        let employees = self.employee_repository.list_active().await?;
        match resolve_registration_match(message, &employees) {
            RegistrationEmployeeMatch::Unique(employee) => {
                let Some(employee_id) = employee.id else {
                    return Err(AppError::internal(
                        "EMPLOYEE_ID_MISSING",
                        "Directory employee must contain an identifier before linking registration",
                        json!({ "full_name": employee.full_name }),
                    ));
                };
                Ok(RegistrationLinkPreview::Ready(
                    RegistrationLinkDecision::EmployeeId(employee_id),
                ))
            }
            RegistrationEmployeeMatch::Ambiguous(candidates) => Ok(
                RegistrationLinkPreview::ClarificationRequired(RegistrationLinkClarification {
                    message: REGISTRATION_AMBIGUOUS_MESSAGE.to_owned(),
                    candidates: candidates_from_matches(&candidates),
                    allow_continue_unlinked: true,
                }),
            ),
            RegistrationEmployeeMatch::NotFound => Ok(RegistrationLinkPreview::Ready(
                RegistrationLinkDecision::ContinueUnlinked,
            )),
        }
    }

    /// Executes registration using an explicit linking `decision` — bypasses
    /// automatic matching.  Used after the user has picked from the clarification
    /// keyboard.
    pub async fn execute_with_link_decision(
        &self,
        message: &IncomingMessage,
        decision: RegistrationLinkDecision,
    ) -> AppResult<User> {
        let employee = self
            .resolve_employee_for_decision(message, decision)
            .await?;
        let actor = self.persist_user(message, employee.as_ref()).await?;

        self.recover_pending_assignments(&actor, employee.as_ref())
            .await?;
        Ok(actor)
    }

    // ─── Private ─────────────────────────────────────────────────────────────

    async fn resolve_employee_for_decision(
        &self,
        message: &IncomingMessage,
        decision: RegistrationLinkDecision,
    ) -> AppResult<Option<Employee>> {
        match decision {
            RegistrationLinkDecision::ContinueUnlinked => Ok(None),
            RegistrationLinkDecision::EmployeeId(employee_id) => {
                self.employee_repository.find_by_id(employee_id).await
            }
            RegistrationLinkDecision::Auto => match self.preview_linking(message).await? {
                RegistrationLinkPreview::Ready(RegistrationLinkDecision::EmployeeId(
                    employee_id,
                )) => self.employee_repository.find_by_id(employee_id).await,
                RegistrationLinkPreview::Ready(RegistrationLinkDecision::ContinueUnlinked) => {
                    Ok(None)
                }
                RegistrationLinkPreview::Ready(RegistrationLinkDecision::Auto)
                | RegistrationLinkPreview::ClarificationRequired(_) => {
                    Err(AppError::business_rule(
                        "REGISTRATION_LINK_DECISION_REQUIRED",
                        "Registration requires an explicit linking decision",
                        json!({ "telegram_id": message.sender_id }),
                    ))
                }
            },
        }
    }

    async fn persist_user(
        &self,
        message: &IncomingMessage,
        employee: Option<&Employee>,
    ) -> AppResult<User> {
        let mut user = User::from_message(message, UserRole::User, employee.is_some());
        user.linked_employee_id = employee.and_then(|value| value.id);
        self.user_repository.upsert_from_message(&user).await
    }

    // ─── Registration recovery ────────────────────────────────────────────────

    /// Links any open tasks that were assigned to `employee` before they registered
    /// to the newly-created user account.
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

    // ─── Audit + notifications ────────────────────────────────────────────────

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

// ─── Private helpers ──────────────────────────────────────────────────────────

/// Returns `true` when the incoming message is an explicit `/start` command,
/// which signals the user wants to retry employee linking.
fn is_explicit_start_command(message: &IncomingMessage) -> bool {
    message
        .text_payload()
        .is_some_and(|payload| payload.trim_start().starts_with("/start"))
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
