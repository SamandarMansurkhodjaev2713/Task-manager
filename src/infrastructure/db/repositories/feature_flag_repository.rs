use std::collections::HashMap;

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::application::ports::repositories::FeatureFlagRepository;
use crate::domain::errors::AppResult;
use crate::shared::feature_flags::FeatureFlag;

use super::common::database_error;

/// SQLite-backed [`FeatureFlagRepository`] that persists admin-initiated flag
/// overrides in the `feature_flag_overrides` table (migration 007).
///
/// The table uses `flag_key TEXT PRIMARY KEY` so upserts are conflict-free
/// and reads are O(log n) via the primary-key index.
#[derive(Clone)]
pub struct SqliteFeatureFlagRepository {
    pool: SqlitePool,
}

impl SqliteFeatureFlagRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl FeatureFlagRepository for SqliteFeatureFlagRepository {
    async fn list_overrides(&self) -> AppResult<HashMap<FeatureFlag, bool>> {
        // Use the non-macro form: the compile-time DATABASE_URL is not
        // available during `cargo check` in this environment.
        let rows = sqlx::query("SELECT flag_key, enabled FROM feature_flag_overrides")
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;

        let mut result = HashMap::new();
        for row in rows {
            use sqlx::Row;
            let flag_key: String = row.get("flag_key");
            let enabled: i64 = row.get("enabled");
            // Skip rows whose key no longer maps to a known variant; this
            // allows a newer DB to survive a rollback to an older binary.
            if let Ok(flag) = flag_key.parse::<FeatureFlag>() {
                result.insert(flag, enabled != 0);
            }
        }
        Ok(result)
    }

    async fn upsert_override(
        &self,
        flag: FeatureFlag,
        enabled: bool,
        updated_by_user_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> AppResult<()> {
        sqlx::query(
            "INSERT INTO feature_flag_overrides
                 (flag_key, enabled, updated_by_user_id, updated_at)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(flag_key) DO UPDATE SET
                 enabled            = excluded.enabled,
                 updated_by_user_id = excluded.updated_by_user_id,
                 updated_at         = excluded.updated_at",
        )
        .bind(flag.as_key())
        .bind(if enabled { 1i64 } else { 0i64 })
        .bind(updated_by_user_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(())
    }
}
