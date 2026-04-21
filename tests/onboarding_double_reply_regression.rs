//! Regression tests for the P0 "double reply after onboarding completion" bug.
//!
//! These tests exercise the **use-case level** invariants that the dispatcher
//! relies on to decide between `RegistrationResult::Ready(actor)` and
//! `RegistrationResult::ConsumedByOnboarding`.  Together with the unit tests
//! in [`src/presentation/telegram/gateway.rs`] they prove that:
//!
//! 1. `OnboardingUseCase` faithfully reports `Completed { user }` on the exact
//!    update that closes onboarding, so the dispatcher can return *before*
//!    running any business handler on the same raw message payload.
//! 2. After completion, a subsequent `probe_onboarding_state` returns a user
//!    whose `onboarding_state == Completed` and whose `first_name` /
//!    `last_name` are both populated — which is the `NotApplicable` fast path
//!    the gate takes for every follow-up update.
//! 3. `handle_text` on a `Completed` user is **idempotent**: it must not
//!    panic or mutate state; the dispatcher has to see `Completed` and early
//!    return on every re-delivery of the webhook.

use std::sync::Arc;

use chrono::Utc;
use tempfile::{tempdir, TempDir};

use telegram_task_bot::application::context::{PrincipalContext, TelegramChatContext};
use telegram_task_bot::application::use_cases::onboarding::{
    OnboardingOutcome, OnboardingTextInput, OnboardingUseCase,
};
use telegram_task_bot::application::use_cases::register_user::RegisterUserUseCase;
use telegram_task_bot::domain::message::{IncomingMessage, MessageContent};
use telegram_task_bot::domain::user::OnboardingState;
use telegram_task_bot::infrastructure::clock::system_clock::SystemClock;
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::{
    SqliteAuditLogRepository, SqliteEmployeeRepository, SqliteNotificationRepository,
    SqliteTaskRepository, SqliteUserRepository,
};

#[tokio::test]
async fn given_user_finishes_onboarding_when_last_name_submitted_then_use_case_reports_completed() {
    let (_tmp, pool) = test_pool().await;

    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let employee_repository = Arc::new(SqliteEmployeeRepository::new(pool.clone()));
    let task_repository = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let notification_repository = Arc::new(SqliteNotificationRepository::new(pool.clone()));
    let audit_log_repository = Arc::new(SqliteAuditLogRepository::new(pool.clone()));

    let register_user_use_case = Arc::new(RegisterUserUseCase::new(
        Arc::new(SystemClock),
        user_repository.clone(),
        employee_repository.clone(),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    ));
    let onboarding_use_case = OnboardingUseCase::new(
        user_repository.clone(),
        employee_repository,
        register_user_use_case,
    );

    // Step 1: /start creates the session row and asks for the first name.
    let start_message = new_message("/start");
    let ctx = anonymous_ctx(&start_message);
    let first_outcome = onboarding_use_case
        .handle_start(&ctx, &start_message)
        .await
        .expect("onboarding session should bootstrap");
    assert!(
        matches!(first_outcome, OnboardingOutcome::AskFirstName { .. }),
        "initial outcome should be AskFirstName, got {:?}",
        first_outcome
    );

    // Step 2: user types their first name.
    let first_name_message = new_message("Самандар");
    let ctx = anonymous_ctx(&first_name_message);
    let outcome_after_first = onboarding_use_case
        .handle_text(
            &ctx,
            &first_name_message,
            OnboardingTextInput {
                text: "Самандар".to_owned(),
            },
        )
        .await
        .expect("first_name step should succeed");
    assert!(
        matches!(outcome_after_first, OnboardingOutcome::AskLastName { .. }),
        "after first_name the FSM should advance to AskLastName, got {:?}",
        outcome_after_first
    );

    // Step 3: user types their last name.  preview_linking will return
    // `Ready(ContinueUnlinked)` because no employee matches — which is the
    // branch that finishes onboarding right away and produced the original
    // double-reply bug.
    let last_name_message = new_message("Мансурходжаев");
    let ctx = anonymous_ctx(&last_name_message);
    let outcome_after_last = onboarding_use_case
        .handle_text(
            &ctx,
            &last_name_message,
            OnboardingTextInput {
                text: "Мансурходжаев".to_owned(),
            },
        )
        .await
        .expect("last_name step should complete onboarding");
    let completed_user = match outcome_after_last {
        OnboardingOutcome::Completed { user } => user,
        other => panic!(
            "expected Completed after last_name, got {other:?} — this is the exact branch that used to let the dispatcher fall through to create_task_and_present"
        ),
    };

    assert_eq!(
        completed_user.onboarding_state,
        OnboardingState::Completed,
        "Completed outcome must carry a user in the Completed state"
    );
    assert_eq!(
        completed_user.first_name.as_deref(),
        Some("Самандар"),
        "first_name must be persisted so the welcome screen can render it correctly"
    );
    assert_eq!(
        completed_user.last_name.as_deref(),
        Some("Мансурходжаев"),
        "last_name must be persisted so the welcome screen can render it correctly"
    );

    // Invariant used by the gate on the *next* update: probe_onboarding_state
    // returns a user that already looks fully onboarded, so the gate takes
    // the `NotApplicable` branch and the dispatcher resumes normal flow.
    let probed = onboarding_use_case
        .probe_onboarding_state(last_name_message.sender_id)
        .await
        .expect("probe should succeed")
        .expect("user should exist after completion");
    assert_eq!(probed.onboarding_state, OnboardingState::Completed);
    assert!(probed.first_name.as_deref().is_some_and(|v| !v.is_empty()));
    assert!(probed.last_name.as_deref().is_some_and(|v| !v.is_empty()));
}

#[tokio::test]
async fn given_completed_user_when_text_re_submitted_then_onboarding_is_idempotent() {
    // Re-delivered webhooks (or an accidental re-route after the RegistrationResult
    // change is reverted in the future) must not break the use case: a text input
    // arriving at a Completed session must return Completed — never panic.
    let (_tmp, pool) = test_pool().await;

    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let employee_repository = Arc::new(SqliteEmployeeRepository::new(pool.clone()));
    let task_repository = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let notification_repository = Arc::new(SqliteNotificationRepository::new(pool.clone()));
    let audit_log_repository = Arc::new(SqliteAuditLogRepository::new(pool.clone()));

    let register_user_use_case = Arc::new(RegisterUserUseCase::new(
        Arc::new(SystemClock),
        user_repository.clone(),
        employee_repository.clone(),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    ));
    let onboarding_use_case = OnboardingUseCase::new(
        user_repository.clone(),
        employee_repository,
        register_user_use_case,
    );

    // Drive through to completion first.
    let start = new_message("/start");
    let ctx = anonymous_ctx(&start);
    onboarding_use_case
        .handle_start(&ctx, &start)
        .await
        .unwrap();

    let first = new_message("Самандар");
    let ctx = anonymous_ctx(&first);
    onboarding_use_case
        .handle_text(
            &ctx,
            &first,
            OnboardingTextInput {
                text: "Самандар".to_owned(),
            },
        )
        .await
        .unwrap();

    let last = new_message("Мансурходжаев");
    let ctx = anonymous_ctx(&last);
    onboarding_use_case
        .handle_text(
            &ctx,
            &last,
            OnboardingTextInput {
                text: "Мансурходжаев".to_owned(),
            },
        )
        .await
        .unwrap();

    // Now replay the same last-name submission as if Telegram redelivered it.
    let replay = new_message("Мансурходжаев");
    let ctx = anonymous_ctx(&replay);
    let outcome = onboarding_use_case
        .handle_text(
            &ctx,
            &replay,
            OnboardingTextInput {
                text: "Мансурходжаев".to_owned(),
            },
        )
        .await
        .expect("replayed text on Completed session must not error");
    assert!(
        matches!(outcome, OnboardingOutcome::Completed { .. }),
        "replayed text must stay in Completed; got {outcome:?}"
    );
}

// ── helpers ───────────────────────────────────────────────────────────────

async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
    let dir = tempdir().expect("temp dir should be created");
    let db_path = dir.path().join("onboarding-regression.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");
    (dir, pool)
}

fn new_message(payload: &str) -> IncomingMessage {
    let content = if payload.starts_with('/') {
        MessageContent::Command {
            text: payload.to_owned(),
        }
    } else {
        MessageContent::Text {
            text: payload.to_owned(),
        }
    };
    IncomingMessage {
        chat_id: 42,
        message_id: 1,
        sender_id: 100500,
        sender_name: "Killallofthem".to_owned(),
        sender_username: Some("killallofthem".to_owned()),
        content,
        timestamp: Utc::now(),
        source_message_key_override: None,
        is_voice_origin: false,
    }
}

fn anonymous_ctx(msg: &IncomingMessage) -> PrincipalContext {
    PrincipalContext::anonymous(
        TelegramChatContext {
            chat_id: msg.chat_id,
            telegram_user_id: msg.sender_id,
        },
        uuid::Uuid::now_v7(),
        msg.timestamp,
    )
}
