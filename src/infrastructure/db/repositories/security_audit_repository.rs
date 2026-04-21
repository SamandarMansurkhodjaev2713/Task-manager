use sqlx::SqlitePool;

use crate::application::ports::repositories::SecurityAuditLogRepository;
use crate::domain::audit::SecurityAuditEntry;
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::SecurityAuditLogRow;

use super::common::{database_error, serialization_error, SECURITY_AUDIT_COLUMNS};

/// SQLite-backed [`SecurityAuditLogRepository`] that records attempted /
/// denied privileged actions (forbidden calls, callback forgery attempts,
/// rate-limit storms) in the append-only `security_audit_log` table
/// introduced in migration 007.
#[derive(Clone)]
pub struct SqliteSecurityAuditLogRepository {
    pool: SqlitePool,
}

impl SqliteSecurityAuditLogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl SecurityAuditLogRepository for SqliteSecurityAuditLogRepository {
    async fn append(&self, entry: &SecurityAuditEntry) -> AppResult<SecurityAuditEntry> {
        let query = format!(
            "INSERT INTO security_audit_log
                (actor_user_id, telegram_id, event_code, metadata, created_at)
             VALUES (?, ?, ?, ?, ?)
             RETURNING {SECURITY_AUDIT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, SecurityAuditLogRow>(&query)
            .bind(entry.actor_user_id)
            .bind(entry.telegram_id)
            .bind(entry.event_code.as_code())
            .bind(serde_json::to_string(&entry.metadata).map_err(serialization_error)?)
            .bind(entry.created_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_recent(&self, limit: i64) -> AppResult<Vec<SecurityAuditEntry>> {
        let query = format!(
            "SELECT {SECURITY_AUDIT_COLUMNS} FROM security_audit_log
             ORDER BY created_at DESC, id DESC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, SecurityAuditLogRow>(&query)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}
