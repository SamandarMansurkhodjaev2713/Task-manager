//! End-to-end idempotency tests for `transcribe_voice_message`.
//!
//! Verifies the full F-01 contract: a duplicate call with the same Telegram
//! `file_unique_id` MUST NOT charge OpenAI a second time, and MUST return
//! the cached transcript on retry.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};
use serde_json::json;
use tempfile::{tempdir, TempDir};
use uuid::Uuid;

use telegram_task_bot::application::ports::repositories::{
    AssigneeHistoryEntry, AssigneeHistoryRepository, AuditLogRepository, BootstrapPromotion,
    NotificationRepository, PersistedTask, TaskRepository, UserRepository,
};
use telegram_task_bot::application::ports::services::{
    Clock, GeneratedTask, SpeechToTextService, TaskGenerator,
};
use telegram_task_bot::application::use_cases::assignee_resolution::AssigneeResolver;
use telegram_task_bot::application::use_cases::create_task_from_message::{
    CreateTaskFromMessageUseCase, VOICE_TRANSCRIBING_IN_PROGRESS,
};
use telegram_task_bot::domain::audit::AuditLogEntry;
use telegram_task_bot::domain::errors::{AppError, AppResult};
use telegram_task_bot::domain::message::{
    IncomingMessage, MessageContent, ParsedTaskRequest, VoiceAttachment,
};
use telegram_task_bot::domain::notification::{Notification, NotificationType};
use telegram_task_bot::domain::task::{StructuredTaskDraft, Task, TaskStats};
use telegram_task_bot::domain::user::{User, UserRole};
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::{
    SqliteEmployeeRepository, SqliteUserRepository, SqliteVoiceProcessingRepository,
};

// ─── Test doubles ─────────────────────────────────────────────────────────

#[derive(Clone, Default)]
struct CountingSpeechToText {
    calls: Arc<AtomicUsize>,
    canned_response: String,
}

impl CountingSpeechToText {
    fn new(response: &str) -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            canned_response: response.to_owned(),
        }
    }

    fn call_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SpeechToTextService for CountingSpeechToText {
    async fn transcribe(&self, _voice: &VoiceAttachment) -> AppResult<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.canned_response.clone())
    }
}

struct StubClock;

impl Clock for StubClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }
    fn today_utc(&self) -> NaiveDate {
        Utc::now().date_naive()
    }
}

struct StubTaskGenerator;

#[async_trait]
impl TaskGenerator for StubTaskGenerator {
    async fn generate_task(
        &self,
        parsed: &ParsedTaskRequest,
        _assignee: Option<&telegram_task_bot::domain::employee::Employee>,
    ) -> AppResult<GeneratedTask> {
        Ok(GeneratedTask {
            structured_task: StructuredTaskDraft {
                title: parsed.task_description.clone(),
                expected_result: String::new(),
                steps: vec![],
                acceptance_criteria: vec![],
                deadline_iso: None,
                refused: false,
                refusal_reason: None,
            },
            model_name: "stub".to_owned(),
            raw_response: "{}".to_owned(),
        })
    }
}

fn empty_stats() -> TaskStats {
    TaskStats {
        created_count: 0,
        completed_count: 0,
        active_count: 0,
        overdue_count: 0,
        average_completion_hours: None,
    }
}

#[derive(Default)]
struct InMemoryTaskRepository;

#[async_trait]
impl TaskRepository for InMemoryTaskRepository {
    async fn create_if_absent(&self, _task: &Task) -> AppResult<PersistedTask> {
        Err(AppError::internal("UNUSED", "test", json!({})))
    }
    async fn find_by_id(&self, _id: i64) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn find_by_uid(&self, _uid: Uuid) -> AppResult<Option<Task>> {
        Ok(None)
    }
    async fn update(&self, _task: &Task) -> AppResult<Task> {
        Err(AppError::internal("UNUSED", "test", json!({})))
    }
    async fn list_assigned_to_user(
        &self,
        _user_id: i64,
        _cursor: Option<String>,
        _limit: u32,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn list_open_assigned_to_employee_without_user(
        &self,
        _employee_id: i64,
        _limit: i64,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn list_created_by_user(
        &self,
        _user_id: i64,
        _cursor: Option<String>,
        _limit: u32,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn list_all(&self, _cursor: Option<String>, _limit: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_due_between(
        &self,
        _start: NaiveDate,
        _end: NaiveDate,
        _limit: i64,
    ) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn get_overdue(&self, _as_of: NaiveDate, _limit: i64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn count_stats_for_user(&self, _user_id: i64) -> AppResult<TaskStats> {
        Ok(empty_stats())
    }
    async fn count_stats_global(&self) -> AppResult<TaskStats> {
        Ok(empty_stats())
    }
    async fn list_open(&self, _limit: i64) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
    async fn list_active(&self, _cursor: Option<String>, _limit: u32) -> AppResult<Vec<Task>> {
        Ok(vec![])
    }
}

#[derive(Default)]
struct NoopNotificationRepo;

#[async_trait]
impl NotificationRepository for NoopNotificationRepo {
    async fn enqueue(&self, n: &Notification) -> AppResult<Notification> {
        Ok(n.clone())
    }
    async fn list_pending(&self, _limit: i64) -> AppResult<Vec<Notification>> {
        Ok(vec![])
    }
    async fn mark_sent(&self, _id: i64, _msg_id: i32, _at: DateTime<Utc>) -> AppResult<()> {
        Ok(())
    }
    async fn mark_retry_pending(
        &self,
        _id: i64,
        _next: DateTime<Utc>,
        _code: &'static str,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn mark_failed(&self, _id: i64, _code: &'static str) -> AppResult<()> {
        Ok(())
    }
    async fn requeue(&self, _id: i64) -> AppResult<()> {
        Ok(())
    }
    async fn find_latest_for_task_and_recipient(
        &self,
        _task_id: i64,
        _recipient: i64,
        _kind: NotificationType,
    ) -> AppResult<Option<Notification>> {
        Ok(None)
    }
}

#[derive(Default)]
struct NoopAuditLog;

#[async_trait]
impl AuditLogRepository for NoopAuditLog {
    async fn append(&self, entry: &AuditLogEntry) -> AppResult<AuditLogEntry> {
        Ok(entry.clone())
    }
    async fn list_for_task(&self, _task_id: i64) -> AppResult<Vec<AuditLogEntry>> {
        Ok(vec![])
    }
}

#[derive(Default)]
struct NoopHistory;

#[async_trait]
impl AssigneeHistoryRepository for NoopHistory {
    async fn record_assignment(
        &self,
        _creator: i64,
        _employee: i64,
        _now: DateTime<Utc>,
    ) -> AppResult<()> {
        Ok(())
    }
    async fn top_for_creator(
        &self,
        _creator: i64,
        _limit: u32,
    ) -> AppResult<Vec<AssigneeHistoryEntry>> {
        Ok(vec![])
    }
}

// ─── Fixture ──────────────────────────────────────────────────────────────

async fn build_use_case(
    response: &str,
) -> (TempDir, CountingSpeechToText, CreateTaskFromMessageUseCase) {
    let temp = tempdir().expect("temp dir");
    let db_path = temp.path().join("voice_e2e.db");
    let url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&url).await.expect("connect");

    let stt = CountingSpeechToText::new(response);
    let user_repo = Arc::new(SqliteUserRepository::new(pool.clone()));
    let employee_repo = Arc::new(SqliteEmployeeRepository::new(pool.clone()));
    let resolver = Arc::new(
        AssigneeResolver::new(user_repo.clone(), employee_repo.clone())
            .with_history(Arc::new(NoopHistory)),
    );
    let voice_repo = Arc::new(SqliteVoiceProcessingRepository::new(pool.clone()));

    let use_case = CreateTaskFromMessageUseCase::new(
        Arc::new(StubClock),
        user_repo,
        Arc::new(InMemoryTaskRepository),
        Arc::new(NoopNotificationRepo),
        Arc::new(NoopAuditLog),
        Arc::new(StubTaskGenerator),
        Arc::new(stt.clone()),
        resolver,
        voice_repo,
    );
    (temp, stt, use_case)
}

fn voice_message(file_unique_id: &str) -> IncomingMessage {
    IncomingMessage {
        message_id: 100,
        chat_id: 42,
        sender_id: 7,
        sender_name: "Tester".to_owned(),
        sender_username: Some("tester".to_owned()),
        content: MessageContent::Voice {
            voice: VoiceAttachment {
                file_id: "fid".to_owned(),
                file_unique_id: file_unique_id.to_owned(),
                duration_seconds: 5,
                mime_type: Some("audio/ogg".to_owned()),
                file_size_bytes: Some(1024),
            },
        },
        timestamp: Utc::now(),
        source_message_key_override: None,
        is_voice_origin: false,
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────

#[tokio::test]
async fn given_two_sequential_calls_with_same_file_unique_id_then_whisper_called_once() {
    let (_temp, stt, use_case) = build_use_case("hello world").await;
    let msg = voice_message("file-A");

    let first = use_case
        .transcribe_voice_message(&msg, None)
        .await
        .expect("first call should succeed");
    let second = use_case
        .transcribe_voice_message(&msg, None)
        .await
        .expect("second call should hit cache");

    assert_eq!(first, "hello world");
    assert_eq!(second, "hello world", "cached value should match");
    assert_eq!(
        stt.call_count(),
        1,
        "STT must be called exactly once across the two requests"
    );
}

#[tokio::test]
async fn given_concurrent_calls_with_same_file_unique_id_then_one_succeeds_other_busy() {
    let (_temp, stt, use_case) = build_use_case("concurrent").await;
    let use_case = Arc::new(use_case);
    let msg = voice_message("file-B");

    let uc_a = use_case.clone();
    let msg_a = msg.clone();
    let uc_b = use_case.clone();
    let msg_b = msg.clone();

    let (a, b) = tokio::join!(
        async move { uc_a.transcribe_voice_message(&msg_a, None).await },
        async move { uc_b.transcribe_voice_message(&msg_b, None).await },
    );

    let outcomes = [a, b];
    let busy_count = outcomes
        .iter()
        .filter(|res| match res {
            Err(error) => error.code() == VOICE_TRANSCRIBING_IN_PROGRESS,
            Ok(_) => false,
        })
        .count();
    let success_count = outcomes.iter().filter(|res| res.is_ok()).count();

    // The race outcome can vary by SQLite ordering: either both observe
    // the cached `Transcribed` row (1 STT, 2 successes) or one wins the
    // CAS and the other gets BUSY (1 STT, 1 success, 1 busy).  The hard
    // contract is: STT is called at most once.
    assert_eq!(
        stt.call_count(),
        1,
        "STT must be called at most once even under contention"
    );
    assert!(
        success_count >= 1,
        "at least one caller must get the transcript; got busy={busy_count}, success={success_count}"
    );
}

#[tokio::test]
async fn given_distinct_file_unique_ids_then_each_call_hits_stt_independently() {
    let (_temp, stt, use_case) = build_use_case("ok").await;

    use_case
        .transcribe_voice_message(&voice_message("file-X"), None)
        .await
        .expect("first call");
    use_case
        .transcribe_voice_message(&voice_message("file-Y"), None)
        .await
        .expect("second call");

    assert_eq!(
        stt.call_count(),
        2,
        "different keys must not share the cache"
    );
}

// ─── Suppress dead_code lints for unused trait stubs ──────────────────────
//
// Many of the stubs above implement rare repository methods our tests do
// not exercise; explicitly mark the use-case as exercised so clippy's
// `dead_code` lint stays quiet on the `User`/`UserRole` re-imports we keep
// for future test additions.
#[allow(dead_code)]
fn _silence_unused(_u: User, _r: UserRole, _p: BootstrapPromotion, _ur: &dyn UserRepository) {}
