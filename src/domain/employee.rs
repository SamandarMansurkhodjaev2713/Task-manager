use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    pub id: Option<i64>,
    pub full_name: String,
    pub telegram_username: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub is_active: bool,
    pub synced_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EmployeeMatchOutcome {
    Unique(EmployeeMatch),
    Ambiguous(Vec<EmployeeMatch>),
    NotFound,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeeMatch {
    pub employee: Employee,
    pub confidence: u8,
    pub strategy: MatchStrategy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStrategy {
    ExactUsername,
    ExactFullName,
    ExactFirstName,
    SuggestedFirstName,
    SuggestedFullName,
}
