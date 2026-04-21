//! SQLite adapter for [`VoiceProcessingRepository`] — Phase 6 skeleton.
//!
//! The adapter enforces the state-machine contract from
//! [`VoiceProcessingState::can_transition_to`] at the SQL level: every
//! mutation is a compare-and-swap that checks both the row's current state
//! AND the application-level validity of the requested transition.
//!
//! We intentionally do NOT expose the `id` back to the caller as an
//! idempotency key; callers must always use `file_unique_id` — this keeps
//! the key surface stable across migrations (e.g. if we ever swap the PK
//! to a UUID column).

use chrono::{DateTime, Utc};
use sqlx::{Row, SqlitePool};

use crate::application::ports::repositories::{VoiceProcessingRepository, VoiceTransitionOutcome};
use crate::domain::errors::AppResult;
use crate::domain::voice::{VoiceProcessingRecord, VoiceProcessingState};

use super::common::database_error;

#[derive(Clone)]
pub struct SqliteVoiceProcessingRepository {
    pool: SqlitePool,
}

impl SqliteVoiceProcessingRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    async fn fetch_by_key(&self, file_unique_id: &str) -> AppResult<Option<VoiceProcessingRecord>> {
        let row = sqlx::query(
            "SELECT id, file_unique_id, chat_id, telegram_user_id, user_id, state,
                    attempt_count, last_error_code, transcript_preview_hash,
                    completed_at, created_at, updated_at
             FROM voice_processing_records
             WHERE file_unique_id = ?",
        )
        .bind(file_unique_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(database_error)?;

        match row {
            Some(row) => Ok(Some(row_to_record(&row)?)),
            None => Ok(None),
        }
    }
}

#[async_trait::async_trait]
impl VoiceProcessingRepository for SqliteVoiceProcessingRepository {
    async fn get_or_create_queued(
        &self,
        record: &VoiceProcessingRecord,
    ) -> AppResult<VoiceProcessingRecord> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;

        let existing = sqlx::query(
            "SELECT id, file_unique_id, chat_id, telegram_user_id, user_id, state,
                    attempt_count, last_error_code, transcript_preview_hash,
                    completed_at, created_at, updated_at
             FROM voice_processing_records
             WHERE file_unique_id = ?",
        )
        .bind(&record.file_unique_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(database_error)?;

        if let Some(row) = existing {
            tx.commit().await.map_err(database_error)?;
            return row_to_record(&row);
        }

        sqlx::query(
            "INSERT INTO voice_processing_records
                (file_unique_id, chat_id, telegram_user_id, user_id, state,
                 attempt_count, last_error_code, transcript_preview_hash,
                 completed_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, NULL, ?, ?)",
        )
        .bind(&record.file_unique_id)
        .bind(record.chat_id)
        .bind(record.telegram_user_id)
        .bind(record.user_id)
        .bind(record.state.as_code())
        .bind(record.attempt_count as i64)
        .bind(record.created_at)
        .bind(record.updated_at)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;

        let reloaded = sqlx::query(
            "SELECT id, file_unique_id, chat_id, telegram_user_id, user_id, state,
                    attempt_count, last_error_code, transcript_preview_hash,
                    completed_at, created_at, updated_at
             FROM voice_processing_records
             WHERE file_unique_id = ?",
        )
        .bind(&record.file_unique_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;

        tx.commit().await.map_err(database_error)?;
        row_to_record(&reloaded)
    }

    async fn find_by_file_unique_id(
        &self,
        file_unique_id: &str,
    ) -> AppResult<Option<VoiceProcessingRecord>> {
        self.fetch_by_key(file_unique_id).await
    }

    async fn transition_state(
        &self,
        file_unique_id: &str,
        expected: VoiceProcessingState,
        next: VoiceProcessingState,
        error_code: Option<&str>,
        now: DateTime<Utc>,
    ) -> AppResult<VoiceTransitionOutcome> {
        if !expected.can_transition_to(next) {
            return Ok(VoiceTransitionOutcome::InvalidTransition);
        }

        let mut tx = self.pool.begin().await.map_err(database_error)?;

        let current =
            sqlx::query("SELECT state FROM voice_processing_records WHERE file_unique_id = ?")
                .bind(file_unique_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database_error)?;

        let Some(row) = current else {
            tx.commit().await.map_err(database_error)?;
            return Ok(VoiceTransitionOutcome::NotFound);
        };

        let current_state_code: String = row.try_get("state").map_err(database_error)?;
        if current_state_code != expected.as_code() {
            tx.commit().await.map_err(database_error)?;
            return Ok(VoiceTransitionOutcome::StaleExpectedState);
        }

        sqlx::query(
            "UPDATE voice_processing_records
             SET state = ?,
                 attempt_count = attempt_count + 1,
                 last_error_code = ?,
                 updated_at = ?
             WHERE file_unique_id = ?",
        )
        .bind(next.as_code())
        .bind(error_code)
        .bind(now)
        .bind(file_unique_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;

        let reloaded = sqlx::query(
            "SELECT id, file_unique_id, chat_id, telegram_user_id, user_id, state,
                    attempt_count, last_error_code, transcript_preview_hash,
                    completed_at, created_at, updated_at
             FROM voice_processing_records
             WHERE file_unique_id = ?",
        )
        .bind(file_unique_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;

        tx.commit().await.map_err(database_error)?;
        Ok(VoiceTransitionOutcome::Transitioned(row_to_record(
            &reloaded,
        )?))
    }

    async fn mark_transcribed(
        &self,
        file_unique_id: &str,
        transcript_preview_hash: &str,
        now: DateTime<Utc>,
    ) -> AppResult<VoiceTransitionOutcome> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;

        let current =
            sqlx::query("SELECT state FROM voice_processing_records WHERE file_unique_id = ?")
                .bind(file_unique_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(database_error)?;

        let Some(row) = current else {
            tx.commit().await.map_err(database_error)?;
            return Ok(VoiceTransitionOutcome::NotFound);
        };

        let current_state_code: String = row.try_get("state").map_err(database_error)?;
        if current_state_code != VoiceProcessingState::Transcribing.as_code() {
            tx.commit().await.map_err(database_error)?;
            return Ok(VoiceTransitionOutcome::StaleExpectedState);
        }

        sqlx::query(
            "UPDATE voice_processing_records
             SET state = 'transcribed',
                 transcript_preview_hash = ?,
                 completed_at = ?,
                 updated_at = ?
             WHERE file_unique_id = ?",
        )
        .bind(transcript_preview_hash)
        .bind(now)
        .bind(now)
        .bind(file_unique_id)
        .execute(&mut *tx)
        .await
        .map_err(database_error)?;

        let reloaded = sqlx::query(
            "SELECT id, file_unique_id, chat_id, telegram_user_id, user_id, state,
                    attempt_count, last_error_code, transcript_preview_hash,
                    completed_at, created_at, updated_at
             FROM voice_processing_records
             WHERE file_unique_id = ?",
        )
        .bind(file_unique_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(database_error)?;

        tx.commit().await.map_err(database_error)?;
        Ok(VoiceTransitionOutcome::Transitioned(row_to_record(
            &reloaded,
        )?))
    }

    async fn purge_stale_payloads(&self, older_than: DateTime<Utc>) -> AppResult<u64> {
        // Only touch terminal records older than the retention window so
        // we never wipe state for an in-flight transcription.
        let result = sqlx::query(
            "UPDATE voice_processing_records
             SET transcript_preview_hash = NULL,
                 last_error_code = NULL
             WHERE completed_at IS NOT NULL
               AND completed_at < ?
               AND state IN ('transcribed', 'failed')
               AND (transcript_preview_hash IS NOT NULL OR last_error_code IS NOT NULL)",
        )
        .bind(older_than)
        .execute(&self.pool)
        .await
        .map_err(database_error)?;

        Ok(result.rows_affected())
    }
}

fn row_to_record(row: &sqlx::sqlite::SqliteRow) -> AppResult<VoiceProcessingRecord> {
    use std::str::FromStr;

    let state_code: String = row.try_get("state").map_err(database_error)?;
    let state = VoiceProcessingState::from_str(&state_code)?;

    Ok(VoiceProcessingRecord {
        id: row
            .try_get::<Option<i64>, _>("id")
            .map_err(database_error)?,
        file_unique_id: row.try_get("file_unique_id").map_err(database_error)?,
        chat_id: row.try_get("chat_id").map_err(database_error)?,
        telegram_user_id: row.try_get("telegram_user_id").map_err(database_error)?,
        user_id: row
            .try_get::<Option<i64>, _>("user_id")
            .map_err(database_error)?,
        state,
        attempt_count: row
            .try_get::<i64, _>("attempt_count")
            .map_err(database_error)?
            .max(0) as u32,
        last_error_code: row
            .try_get::<Option<String>, _>("last_error_code")
            .map_err(database_error)?,
        transcript_preview_hash: row
            .try_get::<Option<String>, _>("transcript_preview_hash")
            .map_err(database_error)?,
        completed_at: row
            .try_get::<Option<DateTime<Utc>>, _>("completed_at")
            .map_err(database_error)?,
        created_at: row.try_get("created_at").map_err(database_error)?,
        updated_at: row.try_get("updated_at").map_err(database_error)?,
    })
}
