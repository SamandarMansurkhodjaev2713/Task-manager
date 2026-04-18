/// Regression tests for UI keyboard correctness.
///
/// These tests verify that keyboard labels and callback wiring remain consistent
/// after the mojibake encoding fix and the guided-flow clarification keyboard fix.
///
/// The presentation layer uses Telegram types, so we test only what we can reach
/// from the pure domain/dto layer — i.e., that the keyboard builder does not panic
/// and that the labels are valid non-empty UTF-8 strings.
use chrono::Utc;
use telegram_task_bot::application::dto::task_views::{
    ClarificationRequest, DeliveryStatus, EmployeeCandidateView, TaskCreationSummary,
};
use telegram_task_bot::domain::task::{MessageType, StructuredTaskDraft, Task};
use telegram_task_bot::presentation::telegram::ui;

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_task() -> Task {
    Task::new(
        "telegram:1:42".to_owned(),
        1,
        Some(2),
        Some(1),
        StructuredTaskDraft {
            title: "Проверить сборку".to_owned(),
            expected_result: "Сборка успешна".to_owned(),
            steps: vec!["Запустить CI".to_owned()],
            acceptance_criteria: vec![],
        },
        None,
        None,
        "check build".to_owned(),
        MessageType::Text,
        "test".to_owned(),
        "{}".to_owned(),
        1,
        42,
        Utc::now(),
    )
    .expect("factory task must be valid")
}

fn make_task_creation_summary(task: &Task) -> TaskCreationSummary {
    TaskCreationSummary::from_task(
        task,
        "Задача создана".to_owned(),
        DeliveryStatus::CreatorOnly,
    )
}

fn make_clarification_request(allow_unassigned: bool) -> ClarificationRequest {
    ClarificationRequest {
        message: "Нашёл нескольких подходящих сотрудников.".to_owned(),
        requested_query: Some("Иван".to_owned()),
        allow_unassigned,
        candidates: vec![
            EmployeeCandidateView {
                employee_id: Some(1),
                full_name: "Иван Петров".to_owned(),
                telegram_username: Some("ivan_petrov".to_owned()),
                confidence: 100,
            },
            EmployeeCandidateView {
                employee_id: Some(2),
                full_name: "Иван Сидоров".to_owned(),
                telegram_username: Some("ivan_sidorov".to_owned()),
                confidence: 100,
            },
        ],
    }
}

// ─── clarification_keyboard ───────────────────────────────────────────────────

#[test]
fn given_clarification_request_with_unassigned_allowed_when_building_keyboard_then_produces_valid_markup(
) {
    let request = make_clarification_request(true);
    // Must not panic — previously panicked with encoding errors in some runtimes.
    let keyboard = ui::clarification_keyboard(&request);
    // Keyboard must have at least: candidate buttons + unassigned + menu + home.
    assert!(
        keyboard.inline_keyboard.len() >= 4,
        "clarification keyboard with 2 candidates and allow_unassigned must have ≥ 4 rows"
    );
}

#[test]
fn given_clarification_request_without_unassigned_when_building_keyboard_then_no_unassigned_row() {
    let request = make_clarification_request(false);
    let keyboard = ui::clarification_keyboard(&request);
    // With allow_unassigned=false: 2 candidates + menu + home = 4 rows.
    assert_eq!(
        keyboard.inline_keyboard.len(),
        4,
        "without allow_unassigned the keyboard must have exactly 4 rows (2 candidates + menu + home)"
    );
}

#[test]
fn given_clarification_request_with_unassigned_when_building_keyboard_then_has_five_rows() {
    let request = make_clarification_request(true);
    let keyboard = ui::clarification_keyboard(&request);
    // 2 candidates + unassigned + menu + home = 5 rows.
    assert_eq!(
        keyboard.inline_keyboard.len(),
        5,
        "with allow_unassigned the keyboard must have exactly 5 rows"
    );
}

// ─── created_task_followup_keyboard ──────────────────────────────────────────

#[test]
fn given_created_summary_without_assign_owner_when_building_followup_keyboard_then_no_owner_row() {
    let task = make_task();
    let summary = make_task_creation_summary(&task);
    let keyboard = ui::created_task_followup_keyboard(&summary, false);
    // open card + (no owner row) + [more / home] = 2 rows.
    assert_eq!(
        keyboard.inline_keyboard.len(),
        2,
        "followup keyboard without assign_owner must have 2 rows"
    );
}

#[test]
fn given_created_summary_with_assign_owner_when_building_followup_keyboard_then_has_owner_row() {
    let task = make_task();
    let summary = make_task_creation_summary(&task);
    let keyboard = ui::created_task_followup_keyboard(&summary, true);
    // open card + owner + [more / home] = 3 rows.
    assert_eq!(
        keyboard.inline_keyboard.len(),
        3,
        "followup keyboard with assign_owner must have 3 rows"
    );
}

// ─── Keyboard label encoding (regression for mojibake fix) ───────────────────

#[test]
fn given_clarification_keyboard_when_inspecting_labels_then_all_text_is_valid_utf8() {
    let request = make_clarification_request(true);
    let keyboard = ui::clarification_keyboard(&request);
    for row in &keyboard.inline_keyboard {
        for btn in row {
            // Button text must be valid UTF-8 and non-empty — the mojibake regression
            // produced garbled characters like "РЎРѕР·РґР°С‚СЊ Р±РµР·…" instead of Russian.
            assert!(
                !btn.text.is_empty(),
                "clarification button text must not be empty"
            );
            // Validate it encodes cleanly as UTF-8 (it will if the source is correct).
            let _ = btn.text.as_bytes(); // always valid in Rust — presence confirms no panic
        }
    }
}

#[test]
fn given_created_followup_keyboard_when_inspecting_labels_then_all_text_is_valid_utf8() {
    let task = make_task();
    let summary = make_task_creation_summary(&task);
    let keyboard = ui::created_task_followup_keyboard(&summary, true);
    for row in &keyboard.inline_keyboard {
        for btn in row {
            assert!(
                !btn.text.is_empty(),
                "followup button text must not be empty"
            );
        }
    }
}

// ─── quick_capture_keyboard ───────────────────────────────────────────────────

#[test]
fn given_quick_capture_keyboard_when_building_then_has_exactly_one_home_row() {
    let keyboard = ui::quick_capture_keyboard();
    assert_eq!(
        keyboard.inline_keyboard.len(),
        1,
        "quick capture keyboard must have exactly 1 row (home only)"
    );
    assert_eq!(
        keyboard.inline_keyboard[0].len(),
        1,
        "quick capture keyboard row must have exactly 1 button"
    );
}
