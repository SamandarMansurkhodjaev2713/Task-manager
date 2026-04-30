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

/// Provides a compact "Full Name — @username" roster digest for the AI
/// prompt context.  Kept as a dedicated port so the AI infrastructure
/// never reaches into [`EmployeeRepository`] directly (clean architecture
/// boundary) and can be faked/stubbed in tests without spinning up a
/// database.
///
/// The returned string is PII-light (no phone numbers, no email), bounded
/// in size by the implementation, and MAY be empty (the prompt treats
/// empty as "no roster available").
#[async_trait]
pub trait DirectoryDigestProvider: Send + Sync {
    async fn fetch_digest(&self) -> AppResult<String>;
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

/// Write-back gateway: appends a row representing a `bot_registered` employee
/// to the Google Sheets directory so that operator-managed spreadsheets stay
/// in sync after a user self-registers through the bot.
///
/// The concrete adapter is [`GoogleSheetsWriteBackClient`].  When write-back
/// is not configured (missing `GOOGLE_SHEETS_WRITE_BACK_RANGE` or
/// `GOOGLE_SHEETS_BEARER_TOKEN`), the application wires in a no-op stub so
/// the rest of the codebase never has to check for `None`.
#[async_trait]
pub trait SheetsWriteBackGateway: Send + Sync {
    /// Appends one employee row to the configured write-back range.
    /// Implementations are responsible for rate limiting and retries; the
    /// use case layer only calls this once per pending row per flush cycle.
    async fn append_employee_row(
        &self,
        full_name: &str,
        telegram_username: Option<&str>,
        telegram_id: i64,
    ) -> AppResult<()>;
}
