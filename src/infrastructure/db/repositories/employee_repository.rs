use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::application::ports::repositories::EmployeeRepository;
use crate::domain::employee::{Employee, WorkloadSnapshot};
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::EmployeeRow;

use super::common::{bool_as_i64, database_error, EMPLOYEE_COLUMNS};

#[derive(Clone)]
pub struct SqliteEmployeeRepository {
    pool: SqlitePool,
}

impl SqliteEmployeeRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl EmployeeRepository for SqliteEmployeeRepository {
    /// Upserts a batch of employees from Google Sheets inside a single
    /// transaction.  Every row written by this method is tagged
    /// `source = 'google_sheets'`, which also "upgrades" any pre-existing
    /// `bot_registered` row whose owner later appears in the Sheets directory.
    ///
    /// **Same-name collision handling:**
    ///
    /// - With a `telegram_username`: matched via the partial unique index;
    ///   upgrades `bot_registered` rows automatically.
    /// - Without a `telegram_username`: find an existing row with the same
    ///   `full_name` and no username, or insert a fresh row.
    async fn upsert_many(&self, employees: &[Employee]) -> AppResult<usize> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;

        for employee in employees {
            if employee.telegram_username.is_some() {
                // Has a Telegram username → use the partial unique index.
                // `source` is forced to 'google_sheets' to upgrade any
                // `bot_registered` row that was created before this sync ran.
                sqlx::query(
                    "INSERT INTO employees
                         (full_name, telegram_username, email, phone, department,
                          is_active, source, synced_at, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, 'google_sheets', ?, ?, ?)
                     ON CONFLICT(telegram_username) WHERE telegram_username IS NOT NULL
                     DO UPDATE SET
                         full_name        = excluded.full_name,
                         email            = excluded.email,
                         phone            = excluded.phone,
                         department       = excluded.department,
                         is_active        = excluded.is_active,
                         source           = 'google_sheets',
                         synced_at        = excluded.synced_at,
                         updated_at       = excluded.updated_at",
                )
                .bind(&employee.full_name)
                .bind(employee.telegram_username.as_deref())
                .bind(employee.email.as_deref())
                .bind(employee.phone.as_deref())
                .bind(employee.department.as_deref())
                .bind(bool_as_i64(employee.is_active))
                .bind(employee.synced_at)
                .bind(employee.created_at)
                .bind(employee.updated_at)
                .execute(&mut *transaction)
                .await
                .map_err(database_error)?;
            } else {
                // No Telegram username — find an existing nameless row with the
                // same full_name and update it, or insert a fresh row.
                let existing_id = sqlx::query_scalar::<_, i64>(
                    "SELECT id FROM employees
                     WHERE full_name = ? AND telegram_username IS NULL
                     LIMIT 1",
                )
                .bind(&employee.full_name)
                .fetch_optional(&mut *transaction)
                .await
                .map_err(database_error)?;

                if let Some(id) = existing_id {
                    sqlx::query(
                        "UPDATE employees SET
                             email       = ?,
                             phone       = ?,
                             department  = ?,
                             is_active   = ?,
                             source      = 'google_sheets',
                             synced_at   = ?,
                             updated_at  = ?
                         WHERE id = ?",
                    )
                    .bind(employee.email.as_deref())
                    .bind(employee.phone.as_deref())
                    .bind(employee.department.as_deref())
                    .bind(bool_as_i64(employee.is_active))
                    .bind(employee.synced_at)
                    .bind(employee.updated_at)
                    .bind(id)
                    .execute(&mut *transaction)
                    .await
                    .map_err(database_error)?;
                } else {
                    sqlx::query(
                        "INSERT INTO employees
                             (full_name, telegram_username, email, phone, department,
                              is_active, source, synced_at, created_at, updated_at)
                         VALUES (?, NULL, ?, ?, ?, ?, 'google_sheets', ?, ?, ?)",
                    )
                    .bind(&employee.full_name)
                    .bind(employee.email.as_deref())
                    .bind(employee.phone.as_deref())
                    .bind(employee.department.as_deref())
                    .bind(bool_as_i64(employee.is_active))
                    .bind(employee.synced_at)
                    .bind(employee.created_at)
                    .bind(employee.updated_at)
                    .execute(&mut *transaction)
                    .await
                    .map_err(database_error)?;
                }
            }
        }

        transaction.commit().await.map_err(database_error)?;
        Ok(employees.len())
    }

    async fn list_active(&self) -> AppResult<Vec<Employee>> {
        let query = format!(
            "SELECT {EMPLOYEE_COLUMNS} FROM employees WHERE is_active = 1 ORDER BY full_name ASC"
        );
        let rows = sqlx::query_as::<_, EmployeeRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn find_by_id(&self, employee_id: i64) -> AppResult<Option<Employee>> {
        let query = format!("SELECT {EMPLOYEE_COLUMNS} FROM employees WHERE id = ?");
        let row = sqlx::query_as::<_, EmployeeRow>(&query)
            .bind(employee_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(row.map(Into::into))
    }

    async fn find_by_telegram_username(&self, username: &str) -> AppResult<Option<Employee>> {
        // Use the partial unique index for an O(1) lookup.
        // Strip a leading '@' so callers can pass either form.
        let normalized = username.trim_start_matches('@');
        let query = format!(
            "SELECT {EMPLOYEE_COLUMNS} FROM employees
             WHERE lower(telegram_username) = lower(?) AND telegram_username IS NOT NULL
             LIMIT 1"
        );
        let row = sqlx::query_as::<_, EmployeeRow>(&query)
            .bind(normalized)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(row.map(Into::into))
    }

    /// Inserts a `bot_registered` employee created during `/start` onboarding.
    ///
    /// Dedup logic:
    /// - If `telegram_username` is set and an employee with that username already
    ///   exists (any source), that existing record is returned — we never create
    ///   a duplicate for the same Telegram handle.
    /// - If `telegram_username` is `None`, a new row is always inserted because
    ///   we have no reliable dedup key.
    async fn upsert_bot_registered(
        &self,
        full_name: &str,
        telegram_username: Option<&str>,
        now: DateTime<Utc>,
    ) -> AppResult<Employee> {
        // Fast path: if we already have an employee with this username (likely
        // from a prior Sheets sync), just return it to avoid duplicate rows.
        if let Some(username) = telegram_username {
            if let Some(existing) = self.find_by_telegram_username(username).await? {
                return Ok(existing);
            }
        }

        let normalized_username = telegram_username.map(|u| u.trim_start_matches('@'));
        let query = format!(
            "INSERT INTO employees
                 (full_name, telegram_username, source, is_active, created_at, updated_at)
             VALUES (?, ?, 'bot_registered', 1, ?, ?)
             RETURNING {EMPLOYEE_COLUMNS}"
        );
        let row = sqlx::query_as::<_, EmployeeRow>(&query)
            .bind(full_name)
            .bind(normalized_username)
            .bind(now)
            .bind(now)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(row.into())
    }

    async fn reset_all(&self) -> AppResult<u64> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;

        // Count up-front so the caller can log a meaningful number.
        let pre_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM employees")
            .fetch_one(&mut *transaction)
            .await
            .map_err(database_error)?;

        // Two FK pointers do NOT have ON DELETE SET NULL — we must clear
        // them manually before wiping `employees`.  Migrations 016
        // already cascaded the rest (person_trigrams, pending_sheet_writes,
        // assignee_history, aliases) via ON DELETE CASCADE.
        sqlx::query("UPDATE tasks SET assigned_to_employee_id = NULL WHERE assigned_to_employee_id IS NOT NULL")
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;
        sqlx::query(
            "UPDATE users SET linked_employee_id = NULL WHERE linked_employee_id IS NOT NULL",
        )
        .execute(&mut *transaction)
        .await
        .map_err(database_error)?;

        sqlx::query("DELETE FROM employees")
            .execute(&mut *transaction)
            .await
            .map_err(database_error)?;

        transaction.commit().await.map_err(database_error)?;

        Ok(pre_count.0.max(0) as u64)
    }

    /// Counts non-terminal (active) tasks assigned to this employee.
    ///
    /// Active = status NOT IN ('completed', 'cancelled').
    /// Overdue = active AND deadline < today (UTC).
    ///
    /// Both queries run in a single SQL SELECT so the snapshot is
    /// point-in-time consistent within a single SQLite read.
    async fn workload_snapshot(&self, employee_id: i64) -> AppResult<WorkloadSnapshot> {
        // Single-query: count total active + overdue active in one pass using
        // conditional aggregation.  This avoids two round-trips to the DB.
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT
                 COUNT(*) FILTER (WHERE status NOT IN ('completed','cancelled')) AS active,
                 COUNT(*) FILTER (
                     WHERE status NOT IN ('completed','cancelled')
                       AND deadline IS NOT NULL
                       AND deadline < date('now')
                 ) AS overdue
             FROM tasks
             WHERE assigned_to_employee_id = ?",
        )
        .bind(employee_id)
        .fetch_one(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(WorkloadSnapshot {
            employee_id,
            active_task_count: row.0.max(0) as u32,
            overdue_task_count: row.1.max(0) as u32,
        })
    }
}

// ─── Unit tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use crate::domain::employee::{Employee, EmployeeSource};

    /// Ensures the `EmployeeSource::as_str` round-trip matches what the DB CHECK
    /// constraint accepts.  This is a pure-logic test — no DB required.
    #[test]
    fn given_employee_source_when_as_str_then_matches_db_check_values() {
        assert_eq!(EmployeeSource::GoogleSheets.as_str(), "google_sheets");
        assert_eq!(EmployeeSource::BotRegistered.as_str(), "bot_registered");
    }

    /// Verifies that parsing the `'google_sheets'` sentinel from a DB row always
    /// produces the correct variant, not the fallback.
    #[test]
    fn given_google_sheets_string_when_parse_then_produces_correct_source() {
        use crate::infrastructure::db::models::EmployeeRow;
        use chrono::Utc;

        let row = EmployeeRow {
            id: 1,
            full_name: "Test User".to_owned(),
            telegram_username: None,
            email: None,
            phone: None,
            department: None,
            is_active: 1,
            source: "google_sheets".to_owned(),
            synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let employee: Employee = row.into();
        assert_eq!(employee.source, EmployeeSource::GoogleSheets);
    }

    /// Verifies that parsing the `'bot_registered'` sentinel from a DB row
    /// produces the `BotRegistered` variant.
    #[test]
    fn given_bot_registered_string_when_parse_then_produces_correct_source() {
        use crate::infrastructure::db::models::EmployeeRow;
        use chrono::Utc;

        let row = EmployeeRow {
            id: 2,
            full_name: "Bot User".to_owned(),
            telegram_username: Some("botuser".to_owned()),
            email: None,
            phone: None,
            department: None,
            is_active: 1,
            source: "bot_registered".to_owned(),
            synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let employee: Employee = row.into();
        assert_eq!(employee.source, EmployeeSource::BotRegistered);
    }
}
