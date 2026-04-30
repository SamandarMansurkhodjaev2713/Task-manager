use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::application::ports::repositories::AliasRepository;
use crate::domain::employee::EmployeeAlias;
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::AliasRow;

use super::common::{database_error, ALIAS_COLUMNS};

#[derive(Clone)]
pub struct SqliteAliasRepository {
    pool: SqlitePool,
}

impl SqliteAliasRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AliasRepository for SqliteAliasRepository {
    async fn list_all(&self) -> AppResult<Vec<EmployeeAlias>> {
        let query = format!("SELECT {ALIAS_COLUMNS} FROM employee_aliases ORDER BY id ASC");
        let rows = sqlx::query_as::<_, AliasRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(rows.into_iter().map(Into::into).collect())
    }

    async fn create(
        &self,
        employee_id: i64,
        alias: &str,
        created_by_user_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> AppResult<EmployeeAlias> {
        let query = format!(
            "INSERT INTO employee_aliases (employee_id, alias, created_by_user_id, created_at)
             VALUES (?, ?, ?, ?)
             RETURNING {ALIAS_COLUMNS}"
        );
        let row = sqlx::query_as::<_, AliasRow>(&query)
            .bind(employee_id)
            .bind(alias)
            .bind(created_by_user_id)
            .bind(now)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(row.into())
    }

    async fn delete_for_employee(&self, employee_id: i64) -> AppResult<u64> {
        let result = sqlx::query("DELETE FROM employee_aliases WHERE employee_id = ?")
            .bind(employee_id)
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(result.rows_affected())
    }

    /// Inserts alias rows with `INSERT OR IGNORE` semantics so that repeated
    /// calls (e.g. after every employee sync) are fully idempotent.
    ///
    /// **Important:** the unique index is on `lower(alias)`, so an alias that
    /// was already seeded for *any* employee will simply be skipped.  This is
    /// intentional — it prevents silent overwriting of manually curated aliases
    /// while still making the bulk seed safe to re-run.
    async fn seed_many(&self, pairs: &[(i64, &str)], now: DateTime<Utc>) -> AppResult<usize> {
        if pairs.is_empty() {
            return Ok(0);
        }

        let mut inserted = 0usize;
        // Batch individual inserts inside a transaction for atomicity and speed.
        let mut tx = self.pool.begin().await.map_err(database_error)?;
        for (employee_id, alias) in pairs {
            let result = sqlx::query(
                "INSERT OR IGNORE INTO employee_aliases
                     (employee_id, alias, created_by_user_id, created_at)
                 VALUES (?, ?, NULL, ?)",
            )
            .bind(employee_id)
            .bind(alias)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
            inserted += result.rows_affected() as usize;
        }
        tx.commit().await.map_err(database_error)?;
        Ok(inserted)
    }
}
