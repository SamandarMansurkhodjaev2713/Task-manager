use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A short-form / diminutive / abbreviation that maps to one specific employee.
///
/// The alias is case-insensitive (normalised to lower-case before storage and
/// before lookup).  A unique DB index on `lower(alias)` guarantees that each
/// alias text maps to at most one employee — if two employees would share the
/// same alias (e.g., both nicknamed "Саша"), neither is seeded so the
/// ambiguity stays visible rather than silently routing to the wrong person.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmployeeAlias {
    /// `None` before the row is persisted.
    pub id: Option<i64>,
    pub employee_id: i64,
    /// The alias text, stored in its original case (looked up case-insensitively).
    pub alias: String,
    /// Which bot user created this alias row (None for system-seeded rows).
    pub created_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
}

/// Lightweight workload snapshot for one employee, used to annotate the
/// "confirm assignee" keyboard with context ("3 active tasks").
///
/// Populated by [`EmployeeRepository::workload_snapshot`] which queries the
/// tasks table directly.  Returned as zeros when the repository does not
/// override the default implementation (test stubs, local directory).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkloadSnapshot {
    pub employee_id: i64,
    /// Number of non-terminal tasks currently assigned to this employee.
    pub active_task_count: u32,
    /// Subset of active tasks whose deadline has already passed.
    pub overdue_task_count: u32,
}

/// Where an employee record originated from.
///
/// `GoogleSheets` rows are owned by the Sheets sync: every `SyncEmployeesUseCase`
/// run may update them.  `BotRegistered` rows are created by the onboarding
/// flow when a user completes `/start` without matching any Sheets entry, so
/// that the user is still discoverable for task assignment.
///
/// The Sheets sync "upgrades" a `BotRegistered` row to `GoogleSheets` if the
/// same person later appears in the Sheets directory (matched by
/// `telegram_username`), making Sheets the authoritative source once it knows
/// about the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmployeeSource {
    GoogleSheets,
    BotRegistered,
}

impl EmployeeSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::GoogleSheets => "google_sheets",
            Self::BotRegistered => "bot_registered",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Employee {
    pub id: Option<i64>,
    pub full_name: String,
    pub telegram_username: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub department: Option<String>,
    pub is_active: bool,
    /// Where this record was originally created from.
    pub source: EmployeeSource,
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
    /// The query matched a registered alias (diminutive / abbreviation) for
    /// exactly this employee.  Confidence is set to `ALIAS_MATCH_CONFIDENCE`
    /// (92) — below `HIGH_CONFIDENCE_THRESHOLD` so confirmation is always
    /// shown rather than auto-assigning based on a short-form name.
    ExactAlias,
    /// The query is an unambiguous prefix of exactly one employee's first name
    /// (e.g. "ABD" → "Abdullazi").  Always requires user confirmation — never
    /// auto-assigned — because partial names have higher mis-assignment risk.
    PrefixFirstName,
    SuggestedFirstName,
    SuggestedFullName,
}
