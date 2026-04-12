use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub id: Option<i64>,
    pub task_id: i64,
    pub action: AuditAction,
    pub old_status: Option<String>,
    pub new_status: Option<String>,
    pub changed_by_user_id: Option<i64>,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditAction {
    Created,
    Sent,
    Assigned,
    StatusChanged,
    ReviewRequested,
    Reassigned,
    Blocked,
    Commented,
    Edited,
    Cancelled,
    EmployeesSynced,
}
