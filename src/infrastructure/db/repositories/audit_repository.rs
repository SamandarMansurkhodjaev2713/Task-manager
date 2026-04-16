use sqlx::SqlitePool;

use crate::application::ports::repositories::AuditLogRepository;
use crate::domain::audit::AuditLogEntry;
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::AuditLogRow;

use super::common::{audit_action_to_db, database_error, serialization_error, AUDIT_COLUMNS};

#[derive(Clone)]
pub struct SqliteAuditLogRepository {
    pool: SqlitePool,
}

impl SqliteAuditLogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AuditLogRepository for SqliteAuditLogRepository {
    async fn append(&self, entry: &AuditLogEntry) -> AppResult<AuditLogEntry> {
        let query = format!(
            "INSERT INTO task_history (task_id, action, old_status, new_status, changed_by_user_id, metadata, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             RETURNING {AUDIT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, AuditLogRow>(&query)
            .bind(entry.task_id)
            .bind(audit_action_to_db(entry.action))
            .bind(&entry.old_status)
            .bind(&entry.new_status)
            .bind(entry.changed_by_user_id)
            .bind(serde_json::to_string(&entry.metadata).map_err(serialization_error)?)
            .bind(entry.created_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_for_task(&self, task_id: i64) -> AppResult<Vec<AuditLogEntry>> {
        let query = format!(
            "SELECT {AUDIT_COLUMNS} FROM task_history
             WHERE task_id = ?
             ORDER BY created_at DESC, id DESC"
        );
        let rows = sqlx::query_as::<_, AuditLogRow>(&query)
            .bind(task_id)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}
