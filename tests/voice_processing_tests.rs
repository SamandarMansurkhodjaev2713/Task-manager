//! Integration tests for `SqliteVoiceProcessingRepository` (Phase 6 skeleton).

use chrono::Utc;
use tempfile::{tempdir, TempDir};

use telegram_task_bot::application::ports::repositories::{
    VoiceProcessingRepository, VoiceTransitionOutcome,
};
use telegram_task_bot::domain::voice::{VoiceProcessingRecord, VoiceProcessingState};
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::SqliteVoiceProcessingRepository;

async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
    let temp_dir = tempdir().expect("temp dir should be created");
    let db_path = temp_dir.path().join("voice.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");
    (temp_dir, pool)
}

fn queued_record(key: &str) -> VoiceProcessingRecord {
    VoiceProcessingRecord::queued(key.to_string(), 100, 200, None, Utc::now())
}

#[tokio::test]
async fn given_fresh_queue_call_when_repeated_with_same_key_then_same_record_returned() {
    let (_temp, pool) = test_pool().await;
    let repo = SqliteVoiceProcessingRepository::new(pool);

    let first = repo
        .get_or_create_queued(&queued_record("fu1"))
        .await
        .expect("first create should succeed");
    let second = repo
        .get_or_create_queued(&queued_record("fu1"))
        .await
        .expect("second call must be idempotent");

    assert_eq!(first.id, second.id);
    assert_eq!(second.state, VoiceProcessingState::Queued);
    assert_eq!(second.attempt_count, 0);
}

#[tokio::test]
async fn given_queued_record_when_transitioning_to_transcribing_then_row_updated_once() {
    let (_temp, pool) = test_pool().await;
    let repo = SqliteVoiceProcessingRepository::new(pool);

    repo.get_or_create_queued(&queued_record("fu2"))
        .await
        .expect("create should succeed");

    let outcome = repo
        .transition_state(
            "fu2",
            VoiceProcessingState::Queued,
            VoiceProcessingState::Transcribing,
            None,
            Utc::now(),
        )
        .await
        .expect("transition should succeed");

    match outcome {
        VoiceTransitionOutcome::Transitioned(record) => {
            assert_eq!(record.state, VoiceProcessingState::Transcribing);
            assert_eq!(record.attempt_count, 1);
        }
        other => panic!("expected Transitioned, got {other:?}"),
    }

    // Replaying the same CAS must be rejected as a stale expected state.
    let stale = repo
        .transition_state(
            "fu2",
            VoiceProcessingState::Queued,
            VoiceProcessingState::Transcribing,
            None,
            Utc::now(),
        )
        .await
        .expect("repeat CAS should succeed at the DB layer");
    assert_eq!(stale, VoiceTransitionOutcome::StaleExpectedState);
}

#[tokio::test]
async fn given_queued_when_invalid_transition_then_rejected_without_touching_row() {
    let (_temp, pool) = test_pool().await;
    let repo = SqliteVoiceProcessingRepository::new(pool.clone());

    repo.get_or_create_queued(&queued_record("fu3"))
        .await
        .expect("create should succeed");

    let outcome = repo
        .transition_state(
            "fu3",
            VoiceProcessingState::Queued,
            VoiceProcessingState::Transcribed,
            None,
            Utc::now(),
        )
        .await
        .expect("transition call should not error");

    assert_eq!(outcome, VoiceTransitionOutcome::InvalidTransition);

    let unchanged = repo
        .find_by_file_unique_id("fu3")
        .await
        .expect("find should succeed")
        .expect("record should exist");
    assert_eq!(unchanged.state, VoiceProcessingState::Queued);
    assert_eq!(unchanged.attempt_count, 0);
}

#[tokio::test]
async fn given_transcribing_when_mark_transcribed_then_finalises_with_preview_hash() {
    let (_temp, pool) = test_pool().await;
    let repo = SqliteVoiceProcessingRepository::new(pool);

    repo.get_or_create_queued(&queued_record("fu4"))
        .await
        .expect("create should succeed");
    repo.transition_state(
        "fu4",
        VoiceProcessingState::Queued,
        VoiceProcessingState::Transcribing,
        None,
        Utc::now(),
    )
    .await
    .expect("move to transcribing should succeed");

    let outcome = repo
        .mark_transcribed("fu4", "abc123", Utc::now())
        .await
        .expect("mark_transcribed should succeed");

    match outcome {
        VoiceTransitionOutcome::Transitioned(record) => {
            assert_eq!(record.state, VoiceProcessingState::Transcribed);
            assert_eq!(
                record.transcript_preview_hash.as_deref(),
                Some("abc123"),
                "preview hash must be stored"
            );
            assert!(record.completed_at.is_some(), "completed_at must be set");
        }
        other => panic!("expected Transitioned, got {other:?}"),
    }
}

#[tokio::test]
async fn given_missing_key_when_transition_or_find_then_returns_not_found_variants() {
    let (_temp, pool) = test_pool().await;
    let repo = SqliteVoiceProcessingRepository::new(pool);

    let outcome = repo
        .transition_state(
            "fu-missing",
            VoiceProcessingState::Queued,
            VoiceProcessingState::Transcribing,
            None,
            Utc::now(),
        )
        .await
        .expect("transition call on missing row should not error");
    assert_eq!(outcome, VoiceTransitionOutcome::NotFound);

    let found = repo
        .find_by_file_unique_id("fu-missing")
        .await
        .expect("find should not error");
    assert!(found.is_none());
}
