use sqlx::SqlitePool;

use crate::application::ports::repositories::EmployeeRepository;
use crate::domain::employee::Employee;
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
    async fn upsert_many(&self, employees: &[Employee]) -> AppResult<usize> {
        let mut transaction = self.pool.begin().await.map_err(database_error)?;

        for employee in employees {
            sqlx::query(
                "INSERT INTO employees (full_name, telegram_username, email, phone, department, is_active, synced_at, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT(full_name) DO UPDATE SET
                   telegram_username = excluded.telegram_username,
                   email = excluded.email,
                   phone = excluded.phone,
                   department = excluded.department,
                   is_active = excluded.is_active,
                   synced_at = excluded.synced_at,
                   updated_at = excluded.updated_at",
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
}
