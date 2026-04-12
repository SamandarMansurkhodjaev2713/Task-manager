use std::fmt::{Display, Formatter};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::message::IncomingMessage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    User,
    Manager,
    Admin,
}

impl UserRole {
    pub fn is_admin(self) -> bool {
        matches!(self, Self::Admin)
    }

    pub fn is_manager_or_admin(self) -> bool {
        matches!(self, Self::Manager | Self::Admin)
    }
}

impl Display for UserRole {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::User => "user",
            Self::Manager => "manager",
            Self::Admin => "admin",
        };
        formatter.write_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: Option<i64>,
    pub telegram_id: i64,
    pub last_chat_id: Option<i64>,
    pub telegram_username: Option<String>,
    pub full_name: Option<String>,
    pub is_employee: bool,
    pub role: UserRole,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl User {
    pub fn from_message(message: &IncomingMessage, role: UserRole, is_employee: bool) -> Self {
        let now = message.timestamp;
        Self {
            id: None,
            telegram_id: message.sender_id,
            last_chat_id: Some(message.chat_id),
            telegram_username: message.sender_username.clone(),
            full_name: Some(message.sender_name.clone()),
            is_employee,
            role,
            created_at: now,
            updated_at: now,
        }
    }
}
