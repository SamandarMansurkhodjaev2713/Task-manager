use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::domain::errors::{AppError, AppResult};
use crate::shared::constants::limits::MAX_TASK_COMMENT_LENGTH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommentKind {
    Context,
    Blocker,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComment {
    pub id: Option<i64>,
    pub task_id: i64,
    pub author_user_id: i64,
    pub kind: CommentKind,
    pub body: String,
    pub created_at: DateTime<Utc>,
}

impl TaskComment {
    /// Comments are trimmed before persistence so Telegram/UI rendering stays stable.
    pub fn new(
        task_id: i64,
        author_user_id: i64,
        kind: CommentKind,
        body: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> AppResult<Self> {
        let normalized_body = body.into().trim().to_owned();
        if normalized_body.is_empty() {
            return Err(AppError::business_rule(
                "TASK_COMMENT_EMPTY",
                "Task comment cannot be empty",
                json!({}),
            ));
        }

        if normalized_body.chars().count() > MAX_TASK_COMMENT_LENGTH {
            return Err(AppError::business_rule(
                "TASK_COMMENT_TOO_LONG",
                "Task comment is too long",
                json!({ "limit": MAX_TASK_COMMENT_LENGTH }),
            ));
        }

        Ok(Self {
            id: None,
            task_id,
            author_user_id,
            kind,
            body: normalized_body,
            created_at,
        })
    }
}
