use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use teloxide::types::MessageId;
use uuid::Uuid;

use crate::domain::employee::Employee;
use crate::domain::errors::AppResult;
use crate::domain::message::{ParsedTaskRequest, VoiceAttachment};
use crate::domain::notification::NotificationType;
use crate::domain::task::{StructuredTaskDraft, TaskStatus};

pub trait Clock: Send + Sync {
    fn now_utc(&self) -> DateTime<Utc>;
    fn today_utc(&self) -> NaiveDate;
}

#[async_trait]
pub trait TaskGenerator: Send + Sync {
    async fn generate_task(
        &self,
        parsed_request: &ParsedTaskRequest,
        assignee: Option<&Employee>,
    ) -> AppResult<GeneratedTask>;
}

#[async_trait]
pub trait SpeechToTextService: Send + Sync {
    async fn transcribe(&self, voice: &VoiceAttachment) -> AppResult<String>;
}

#[async_trait]
pub trait EmployeeDirectoryGateway: Send + Sync {
    async fn fetch_employees(&self) -> AppResult<Vec<Employee>>;
}

#[async_trait]
pub trait TelegramNotifier: Send + Sync {
    async fn send_text(&self, chat_id: i64, text: &str) -> AppResult<MessageId>;
    async fn send_notification(
        &self,
        chat_id: i64,
        text: &str,
        notification_type: NotificationType,
        task_uid: Option<Uuid>,
        status: Option<TaskStatus>,
    ) -> AppResult<MessageId>;
}

#[derive(Debug, Clone)]
pub struct GeneratedTask {
    pub model_name: String,
    pub raw_response: String,
    pub structured_task: StructuredTaskDraft,
}
