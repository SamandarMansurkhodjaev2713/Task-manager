use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use validator::Validate;

use crate::domain::errors::{AppError, AppResult};
use crate::shared::constants::limits::MAX_MESSAGE_LENGTH;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text { text: String },
    Voice { voice: VoiceAttachment },
    Command { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct VoiceAttachment {
    #[validate(length(min = 1))]
    pub file_id: String,
    #[validate(length(min = 1))]
    pub file_unique_id: String,
    pub duration_seconds: u32,
    pub mime_type: Option<String>,
    pub file_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct IncomingMessage {
    pub message_id: i32,
    pub chat_id: i64,
    pub sender_id: i64,
    #[validate(length(min = 1))]
    pub sender_name: String,
    pub sender_username: Option<String>,
    pub content: MessageContent,
    pub timestamp: DateTime<Utc>,
    #[serde(default)]
    pub source_message_key_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct ParsedTaskRequest {
    pub assignee_name: Option<String>,
    #[validate(length(min = 1, max = 2_000))]
    pub task_description: String,
    pub deadline: Option<NaiveDate>,
    pub deadline_raw: Option<String>,
    pub explicit_unassigned: bool,
    pub confidence_score: u8,
}

impl IncomingMessage {
    pub fn source_message_key(&self) -> String {
        if let Some(source_message_key) = self.source_message_key_override.as_deref() {
            return source_message_key.to_owned();
        }

        format!("telegram:{}:{}", self.chat_id, self.message_id)
    }

    pub fn text_payload(&self) -> Option<&str> {
        match &self.content {
            MessageContent::Text { text } | MessageContent::Command { text } => Some(text.as_str()),
            MessageContent::Voice { .. } => None,
        }
    }

    pub fn validate_payload_length(&self) -> AppResult<()> {
        if let Some(payload) = self.text_payload() {
            if payload.chars().count() > MAX_MESSAGE_LENGTH {
                return Err(AppError::schema_validation(
                    "MESSAGE_TOO_LONG",
                    "Message exceeds the supported length",
                    json!({ "limit": MAX_MESSAGE_LENGTH }),
                ));
            }
        }

        Ok(())
    }
}
