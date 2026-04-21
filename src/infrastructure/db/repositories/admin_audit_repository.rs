use sqlx::SqlitePool;

use crate::application::ports::repositories::AdminAuditLogRepository;
use crate::domain::audit::AdminAuditEntry;
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::AdminAuditLogRow;

use super::common::{database_error, serialization_error, ADMIN_AUDIT_COLUMNS};

/// SQLite-backed [`AdminAuditLogRepository`] that writes every privileged
/// state change (role promotion, user (de)activation, feature toggle) to
/// the append-only `admin_audit_log` table introduced in migration 007.
#[derive(Clone)]
pub struct SqliteAdminAuditLogRepository {
    pool: SqlitePool,
}

impl SqliteAdminAuditLogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl AdminAuditLogRepository for SqliteAdminAuditLogRepository {
    async fn append(&self, entry: &AdminAuditEntry) -> AppResult<AdminAuditEntry> {
        let query = format!(
            "INSERT INTO admin_audit_log
                (actor_user_id, target_user_id, action_code, metadata, created_at)
             VALUES (?, ?, ?, ?, ?)
             RETURNING {ADMIN_AUDIT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, AdminAuditLogRow>(&query)
            .bind(entry.actor_user_id)
            .bind(entry.target_user_id)
            .bind(entry.action_code.as_code())
            .bind(serde_json::to_string(&entry.metadata).map_err(serialization_error)?)
            .bind(entry.created_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_recent(&self, limit: i64) -> AppResult<Vec<AdminAuditEntry>> {
        let query = format!(
            "SELECT {ADMIN_AUDIT_COLUMNS} FROM admin_audit_log
             ORDER BY created_at DESC, id DESC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, AdminAuditLogRow>(&query)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn list_for_target(
        &self,
        target_user_id: i64,
        limit: i64,
    ) -> AppResult<Vec<AdminAuditEntry>> {
        let query = format!(
            "SELECT {ADMIN_AUDIT_COLUMNS} FROM admin_audit_log
             WHERE target_user_id = ?
             ORDER BY created_at DESC, id DESC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, AdminAuditLogRow>(&query)
            .bind(target_user_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}
