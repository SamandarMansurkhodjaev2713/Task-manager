//! Voice processing domain types (Phase 6 skeleton).
//!
//! These types model a single voice-to-task processing attempt, mirroring
//! the `voice_processing_records` table (migration 010).  The state machine
//! is deliberately small so we can extend it without schema churn:
//!
//! ```text
//!     queued ──► transcribing ──► transcribed
//!        │             │
//!        └─────────────┴───► failed
//! ```
//!
//! Why these types live in the domain layer:
//! * The presentation layer wants to render per-state UI.
//! * The application layer wants to enforce valid transitions without
//!   reaching into SQL.
//! * Tests want to assert on transitions without a database.

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::domain::errors::AppError;

/// Canonical state of a voice-processing attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VoiceProcessingState {
    /// Record freshly inserted; transcription hasn't started yet.
    Queued,
    /// Transcription in flight against the STT provider.
    Transcribing,
    /// Transcription finished successfully and a task draft was produced.
    Transcribed,
    /// Terminal failure; see `last_error_code` for details.
    Failed,
}

impl VoiceProcessingState {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Transcribing => "transcribing",
            Self::Transcribed => "transcribed",
            Self::Failed => "failed",
        }
    }

    /// Returns `true` if the state is terminal and no further transitions
    /// should be attempted.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Transcribed | Self::Failed)
    }

    /// Encodes the transition matrix in a single place.  Callers MUST use
    /// this check before persisting a new state; the repository adapter
    /// also enforces it via an optimistic CAS on `state`.
    pub fn can_transition_to(self, next: Self) -> bool {
        match (self, next) {
            (Self::Queued, Self::Transcribing) => true,
            (Self::Queued, Self::Failed) => true,
            (Self::Transcribing, Self::Transcribed) => true,
            (Self::Transcribing, Self::Failed) => true,
            // Terminal states are sticky.
            _ => false,
        }
    }
}

impl fmt::Display for VoiceProcessingState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_code())
    }
}

impl FromStr for VoiceProcessingState {
    type Err = AppError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "transcribing" => Ok(Self::Transcribing),
            "transcribed" => Ok(Self::Transcribed),
            "failed" => Ok(Self::Failed),
            other => Err(AppError::schema_validation(
                "VOICE_PROCESSING_STATE_UNKNOWN",
                format!("unrecognised voice processing state '{other}'"),
                serde_json::json!({ "state": other }),
            )),
        }
    }
}

/// One voice-processing attempt as persisted in `voice_processing_records`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoiceProcessingRecord {
    pub id: Option<i64>,
    /// Telegram-provided idempotency key.  Stable across webhook retries.
    pub file_unique_id: String,
    pub chat_id: i64,
    pub telegram_user_id: i64,
    /// Internal user primary key once resolved (None for anonymous input).
    pub user_id: Option<i64>,
    pub state: VoiceProcessingState,
    pub attempt_count: u32,
    pub last_error_code: Option<String>,
    pub transcript_preview_hash: Option<String>,
    pub completed_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl VoiceProcessingRecord {
    /// Factory for the initial `queued` row.  Centralises the defaults so
    /// the repository adapter never has to hand-write them.
    pub fn queued(
        file_unique_id: String,
        chat_id: i64,
        telegram_user_id: i64,
        user_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: None,
            file_unique_id,
            chat_id,
            telegram_user_id,
            user_id,
            state: VoiceProcessingState::Queued,
            attempt_count: 0,
            last_error_code: None,
            transcript_preview_hash: None,
            completed_at: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VoiceProcessingRecord, VoiceProcessingState};
    use chrono::Utc;

    #[test]
    fn given_queued_when_transition_to_transcribing_then_allowed() {
        assert!(VoiceProcessingState::Queued.can_transition_to(VoiceProcessingState::Transcribing));
    }

    #[test]
    fn given_terminal_state_when_transition_then_rejected() {
        assert!(!VoiceProcessingState::Transcribed
            .can_transition_to(VoiceProcessingState::Transcribing));
        assert!(!VoiceProcessingState::Failed.can_transition_to(VoiceProcessingState::Queued));
    }

    #[test]
    fn given_queued_factory_when_built_then_sets_expected_defaults() {
        let now = Utc::now();
        let record = VoiceProcessingRecord::queued("fu1".into(), 42, 99, Some(7), now);

        assert_eq!(record.state, VoiceProcessingState::Queued);
        assert_eq!(record.attempt_count, 0);
        assert_eq!(record.completed_at, None);
        assert_eq!(record.created_at, now);
        assert_eq!(record.updated_at, now);
    }

    #[test]
    fn given_bad_state_string_when_parsed_then_returns_validation_error() {
        let err: crate::domain::errors::AppError =
            "garbage".parse::<VoiceProcessingState>().unwrap_err();
        assert_eq!(err.code(), "VOICE_PROCESSING_STATE_UNKNOWN");
    }
}
