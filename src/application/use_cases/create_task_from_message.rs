use std::sync::Arc;
use std::time::Instant;

use metrics::{counter, histogram};
use serde_json::json;
use tracing::instrument;

use crate::application::dto::task_views::{
    DeliveryStatus, TaskCreationOutcome, TaskCreationSummary,
};
use crate::application::ports::repositories::{
    AuditLogRepository, NotificationRepository, PersistedTask, TaskRepository, UserRepository,
};
use crate::application::ports::services::{Clock, SpeechToTextService, TaskGenerator};
use crate::application::use_cases::assignee_resolution::{
    AssigneeResolution, AssigneeResolver, ResolvedAssignee,
};
use crate::domain::audit::{AuditAction, AuditLogEntry};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::parsing::parse_task_request;
use crate::domain::task::{MessageType, Task, TaskStatus};
use crate::domain::user::{User, UserRole};
use crate::shared::task_codes::format_public_task_code_or_placeholder;

pub struct CreateTaskFromMessageUseCase {
    clock: Arc<dyn Clock>,
    user_repository: Arc<dyn UserRepository>,
    task_repository: Arc<dyn TaskRepository>,
    notification_repository: Arc<dyn NotificationRepository>,
    audit_log_repository: Arc<dyn AuditLogRepository>,
    task_generator: Arc<dyn TaskGenerator>,
    speech_to_text_service: Arc<dyn SpeechToTextService>,
    assignee_resolver: Arc<AssigneeResolver>,
}

impl CreateTaskFromMessageUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        clock: Arc<dyn Clock>,
        user_repository: Arc<dyn UserRepository>,
        task_repository: Arc<dyn TaskRepository>,
        notification_repository: Arc<dyn NotificationRepository>,
        audit_log_repository: Arc<dyn AuditLogRepository>,
        task_generator: Arc<dyn TaskGenerator>,
        speech_to_text_service: Arc<dyn SpeechToTextService>,
        assignee_resolver: Arc<AssigneeResolver>,
    ) -> Self {
        Self {
            clock,
            user_repository,
            task_repository,
            notification_repository,
            audit_log_repository,
            task_generator,
            speech_to_text_service,
            assignee_resolver,
        }
    }

    #[instrument(skip_all, fields(chat_id = message.chat_id, message_id = message.message_id))]
    pub async fn execute(&self, message: IncomingMessage) -> AppResult<TaskCreationOutcome> {
        let started_at = Instant::now();
        let message_type_label = message_type_label(&message.content);
        counter!("task_creation_requests_total", "message_type" => message_type_label).increment(1);
        message.validate_payload_length()?;

        let original_text = self.extract_original_text(&message).await?;
        let creator = self.register_creator(&message).await?;
        let parsed_request = parse_task_request(&original_text, self.clock.today_utc())?;
        let assignee_resolution = self.resolve_assignee(&parsed_request).await?;

        let (assignee_user, assignee_employee) = match assignee_resolution {
            AssigneeResolution::Resolved(resolved) => {
                let ResolvedAssignee { user, employee } = *resolved;
                (user, employee)
            }
            AssigneeResolution::ClarificationRequired(request) => {
                return Ok(TaskCreationOutcome::ClarificationRequired(request));
            }
        };

        let generated_task = self
            .task_generator
            .generate_task(&parsed_request, assignee_employee.as_ref())
            .await?;
        let task = Task::new(
            message.source_message_key(),
            required_user_id(&creator)?,
            assignee_user.as_ref().and_then(|user| user.id),
            assignee_employee.as_ref().and_then(|employee| employee.id),
            generated_task.structured_task,
            parsed_request.deadline,
            parsed_request.deadline_raw.clone(),
            original_text,
            task_message_type(&message.content),
            generated_task.model_name,
            generated_task.raw_response,
            message.chat_id,
            message.message_id,
            self.clock.now_utc(),
        )?;

        let persisted_task = self.task_repository.create_if_absent(&task).await?;
        let stored_task = match persisted_task {
            PersistedTask::Created(task) => task,
            PersistedTask::Existing(task) => {
                return Ok(TaskCreationOutcome::DuplicateFound(
                    TaskCreationSummary::from_task(
                        &task,
                        build_duplicate_message(&task),
                        resolve_duplicate_delivery_status(&task),
                    ),
                ));
            }
        };

        self.log_task_creation(
            &stored_task,
            creator.id,
            assignee_user.as_ref().and_then(|user| user.id),
        )
        .await?;
        let delivery_status = self
            .enqueue_assignee_notification(
                &stored_task,
                assignee_user.as_ref(),
                assignee_employee.is_some(),
            )
            .await?;
        histogram!("task_creation_duration_seconds").record(started_at.elapsed().as_secs_f64());

        Ok(TaskCreationOutcome::Created(
            TaskCreationSummary::from_task(
                &stored_task,
                build_creator_message(&stored_task, delivery_status),
                delivery_status,
            ),
        ))
    }

    pub async fn transcribe_voice_message(&self, message: &IncomingMessage) -> AppResult<String> {
        match &message.content {
            MessageContent::Voice { voice } => self.speech_to_text_service.transcribe(voice).await,
            _ => Err(AppError::business_rule(
                "VOICE_MESSAGE_REQUIRED",
                "Voice transcription preview requires a voice message",
                json!({ "message_type": message_type_label(&message.content) }),
            )),
        }
    }

    async fn extract_original_text(&self, message: &IncomingMessage) -> AppResult<String> {
        match &message.content {
            MessageContent::Text { text } | MessageContent::Command { text } => Ok(text.clone()),
            MessageContent::Voice { voice } => self.speech_to_text_service.transcribe(voice).await,
        }
    }

    async fn register_creator(&self, message: &IncomingMessage) -> AppResult<User> {
        let user = User::from_message(message, UserRole::User, false);
        self.user_repository.upsert_from_message(&user).await
    }

    async fn resolve_assignee(
        &self,
        parsed_request: &crate::domain::message::ParsedTaskRequest,
    ) -> AppResult<AssigneeResolution> {
        if parsed_request.explicit_unassigned {
            return Ok(AssigneeResolution::Resolved(Box::new(ResolvedAssignee {
                user: None,
                employee: None,
            })));
        }

        let Some(assignee_query) = parsed_request.assignee_name.as_deref() else {
            return Ok(AssigneeResolution::Resolved(Box::new(ResolvedAssignee {
                user: None,
                employee: None,
            })));
        };

        self.assignee_resolver.resolve(assignee_query).await
    }

    async fn log_task_creation(
        &self,
        task: &Task,
        creator_id: Option<i64>,
        assignee_id: Option<i64>,
    ) -> AppResult<()> {
        let Some(task_id) = task.id else {
            return Err(AppError::internal(
                "TASK_ID_MISSING",
                "Persisted task must contain a database identifier",
                json!({ "task_uid": task.task_uid }),
            ));
        };

        let entry = AuditLogEntry {
            id: None,
            task_id,
            action: AuditAction::Created,
            old_status: None,
            new_status: Some(TaskStatus::Created.to_string()),
            changed_by_user_id: creator_id,
            metadata: json!({
                "assigned_to_user_id": assignee_id,
                "deadline": task.deadline,
            }),
            created_at: self.clock.now_utc(),
        };
        let _ = self.audit_log_repository.append(&entry).await?;
        Ok(())
    }

    async fn enqueue_assignee_notification(
        &self,
        task: &Task,
        assignee_user: Option<&User>,
        has_employee_match: bool,
    ) -> AppResult<DeliveryStatus> {
        let Some(user) = assignee_user else {
            return Ok(if has_employee_match {
                DeliveryStatus::PendingAssigneeRegistration
            } else {
                DeliveryStatus::CreatorOnly
            });
        };

        if user.last_chat_id.is_none() {
            return Ok(DeliveryStatus::PendingAssigneeRegistration);
        }

        let Some(recipient_user_id) = user.id else {
            return Ok(DeliveryStatus::PendingAssigneeRegistration);
        };

        let notification = Notification {
            id: None,
            task_id: task.id,
            recipient_user_id,
            notification_type: NotificationType::TaskAssigned,
            message: task.render_for_telegram(None),
            dedupe_key: format!("task_assigned:{}:{}", task.task_uid, recipient_user_id),
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
        Ok(DeliveryStatus::PendingDelivery)
    }
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

fn task_message_type(content: &MessageContent) -> MessageType {
    match content {
        MessageContent::Voice { .. } => MessageType::Voice,
        MessageContent::Text { .. } | MessageContent::Command { .. } => MessageType::Text,
    }
}

fn build_creator_message(task: &Task, delivery_status: DeliveryStatus) -> String {
    let delivery_hint = match delivery_status {
        DeliveryStatus::DeliveredToAssignee => {
            "Исполнителю уже доставлено уведомление."
        }
        DeliveryStatus::PendingDelivery => {
            "Уведомление поставлено в очередь на отправку исполнителю."
        }
        DeliveryStatus::PendingAssigneeRegistration => {
            "Исполнитель найден, но ещё не запускал бота. После /start уведомления начнут приходить напрямую."
        }
        DeliveryStatus::RetryPending => {
            "Доставка временно не удалась. Бот попробует отправить уведомление повторно."
        }
        DeliveryStatus::Failed => {
            "Задача создана, но доставить уведомление исполнителю пока не получилось."
        }
        DeliveryStatus::CreatorOnly => {
            "Задача создана без прямой доставки исполнителю."
        }
    };

    format!(
        "Задача сохранена.\nID: {}\nСтатус: {}\n{}\n\n{}",
        format_public_task_code_or_placeholder(task.id),
        task.status,
        delivery_hint,
        task.render_for_telegram(None),
    )
}

fn build_duplicate_message(task: &Task) -> String {
    format!(
        "Похоже, это сообщение уже было обработано раньше. Дубликат не создан, открывайте существующую карточку.\n\nID: {}\nСтатус: {}",
        format_public_task_code_or_placeholder(task.id),
        task.status,
    )
}

fn resolve_duplicate_delivery_status(task: &Task) -> DeliveryStatus {
    if task.assigned_to_employee_id.is_some() && task.assigned_to_user_id.is_none() {
        return DeliveryStatus::PendingAssigneeRegistration;
    }

    if task.assigned_to_user_id.is_some() {
        return DeliveryStatus::PendingDelivery;
    }

    DeliveryStatus::CreatorOnly
}

fn message_type_label(content: &MessageContent) -> &'static str {
    match content {
        MessageContent::Text { .. } => "text",
        MessageContent::Voice { .. } => "voice",
        MessageContent::Command { .. } => "command",
    }
}
