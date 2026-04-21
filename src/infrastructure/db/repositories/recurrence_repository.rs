//! SQLite adapter for the recurrence-rule scheduler.
//!
//! Reads rows from `recurrence_rules` and `task_templates` (migration 012).
//! Writes are limited to advancing a rule's schedule after it fires.

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use crate::application::ports::repositories::{
    RecurrenceRepository, RecurrenceRuleRow, TaskTemplateRow,
};
use crate::domain::errors::AppResult;

use super::common::database_error;

#[derive(Clone)]
pub struct SqliteRecurrenceRepository {
    pool: SqlitePool,
}

impl SqliteRecurrenceRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl RecurrenceRepository for SqliteRecurrenceRepository {
    async fn list_due(
        &self,
        as_of: DateTime<Utc>,
        limit: i64,
    ) -> AppResult<Vec<RecurrenceRuleRow>> {
        // Only active rules whose next_run_at has passed are eligible.
        // Rules with next_run_at IS NULL are skipped — they have never been
        // scheduled (either fresh or the CRON produced no future firing time).
        let rows = sqlx::query(
            "SELECT id, template_id, owner_user_id, cron_expression, timezone
             FROM recurrence_rules
             WHERE is_active = 1
               AND next_run_at IS NOT NULL
               AND next_run_at <= ?
             ORDER BY next_run_at ASC
             LIMIT ?",
        )
        .bind(as_of)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(rows
            .iter()
            .map(|row| RecurrenceRuleRow {
                id: row.get("id"),
                template_id: row.get("template_id"),
                owner_user_id: row.get("owner_user_id"),
                cron_expression: row.get("cron_expression"),
                timezone: row.get("timezone"),
            })
            .collect())
    }

    async fn get_template(&self, template_id: i64) -> AppResult<Option<TaskTemplateRow>> {
        let row = sqlx::query(
            "SELECT id, code, title, body, created_by_user_id
             FROM task_templates
             WHERE id = ? AND is_active = 1",
        )
        .bind(template_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(row.map(|r| TaskTemplateRow {
            id: r.get("id"),
            code: r.get("code"),
            title: r.get("title"),
            body: r.get("body"),
            created_by_user_id: r.get("created_by_user_id"),
        }))
    }

    async fn advance_rule(
        &self,
        rule_id: i64,
        fired_at: DateTime<Utc>,
        next_run_at: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE recurrence_rules
             SET last_run_at = ?, next_run_at = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(fired_at)
        .bind(next_run_at)
        .bind(now)
        .bind(rule_id)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;
        Ok(())
    }
}
