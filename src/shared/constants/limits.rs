pub const MAX_MESSAGE_LENGTH: usize = 2_000;
pub const MIN_TASK_DESCRIPTION_LENGTH: usize = 10;
pub const MAX_TASK_TITLE_LENGTH: usize = 100;
/// How much of the task title to show in the compact "✅ Задача создана" reply.
/// Full text remains on the task card.
pub const MAX_TASK_CREATION_CONFIRM_PREVIEW_CHARS: usize = 220;
/// Task body repeated on the assignee-clarification screen so the user does not lose
/// context when Telegram scrolls the original message away.
pub const MAX_CLARIFICATION_TASK_BODY_PREVIEW_CHARS: usize = 400;
pub const MAX_TASK_STEP_LENGTH: usize = 280;
pub const MAX_TASK_EXPECTED_RESULT_LENGTH: usize = 280;
pub const MAX_TASK_ACCEPTANCE_CRITERION_LENGTH: usize = 160;
pub const MAX_TASK_STEPS: usize = 7;
pub const MIN_TASK_STEPS: usize = 1;
pub const MAX_ACCEPTANCE_CRITERIA: usize = 5;
pub const MAX_AUDIO_FILE_SIZE_BYTES: u64 = 25 * 1_024 * 1_024;
pub const MAX_AUDIO_DURATION_SECONDS: u32 = 10 * 60;
/// Upper bound on the transcript we forward to the LLM.  Ten minutes of
/// Russian speech ≈ 1500 words ≈ 12 kB of UTF-8; we keep a 4× safety
/// margin to absorb punctuation-heavy dictation without ever approaching
/// Gemini's 32K context window (P1-voice-pipeline token-budget guard).
pub const MAX_TRANSCRIPT_LENGTH: usize = 6_000;
/// How long we keep the `voice_processing_records` row with any
/// transcript-derived payload before we purge (`completed_at`-based
/// scrub).  Transcript hashes and the row itself are retained for idem-
/// potency + analytics but without any payload after this window.
pub const VOICE_PROCESSING_RETENTION_MINUTES: i64 = 60;
pub const MIN_EMPLOYEE_MATCH_CONFIDENCE: f64 = 0.80;
pub const STRONG_EMPLOYEE_MATCH_CONFIDENCE: f64 = 0.92;
pub const MAX_TASK_COMMENT_LENGTH: usize = 500;
pub const MAX_TASK_BLOCKER_REASON_LENGTH: usize = 280;
pub const MAX_TASK_CONTEXT_PREVIEW_COMMENTS: usize = 3;
pub const MAX_DAILY_SUMMARY_TASKS: usize = 20;
pub const MAX_DAILY_SUMMARY_PREVIEW_TASKS: usize = 5;
pub const DAILY_SUMMARY_OPEN_TASK_SCAN_MULTIPLIER: usize = 10;
pub const REMINDER_TASK_FETCH_LIMIT: i64 = 200;
pub const PUBLIC_TASK_CODE_WIDTH: usize = 4;
pub const MAX_TASK_BUTTON_TITLE_LENGTH: usize = 28;
pub const MANAGER_INBOX_STALE_DAYS: i64 = 3;
pub const REGISTRATION_RECOVERY_TASK_BATCH_SIZE: i64 = 200;
pub const MAX_REGISTRATION_RECOVERY_BATCHES: usize = 5;
