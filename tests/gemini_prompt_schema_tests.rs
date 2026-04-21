//! Integration tests for the Gemini-side of P1-ai-prompt-hardening.
//!
//! We do not exercise the network — the tests fence the contract that
//! our downstream code relies on:
//!
//! 1. `StructuredTaskDraft` accepts `deadline_iso`, `refused` and
//!    `refusal_reason` optionally (the AI may omit them).
//! 2. `refused == true` trips business-rule validation and yields a
//!    stable error code the presentation layer can render.
//! 3. `refused` defaults to `false` so old fixtures keep compiling.
//!
//! The companion deadline kernel tests in `deadline_kernel_tests.rs`
//! cover how the `deadline_iso` value flows through the resolver once
//! `create_task_from_message` plumbs it in.

use telegram_task_bot::domain::errors::AppError;
use telegram_task_bot::domain::task::StructuredTaskDraft;

#[test]
fn given_refused_draft_when_validating_then_returns_business_rule_error() {
    let draft: StructuredTaskDraft = serde_json::from_str(
        r#"{
            "title": "",
            "expected_result": "",
            "steps": [],
            "acceptance_criteria": [],
            "refused": true,
            "refusal_reason": "слишком коротко, уточните задачу"
        }"#,
    )
    .expect("deserialises");

    let err = draft.validate_business_rules().expect_err("must refuse");
    match err {
        AppError::Validation { code, .. } => assert_eq!(code, "TASK_DRAFT_REFUSED"),
        other => panic!("expected validation error, got {other:?}"),
    }
}

#[test]
fn given_new_fields_omitted_when_deserialising_then_defaults_are_safe() {
    let draft: StructuredTaskDraft = serde_json::from_str(
        r#"{
            "title": "Подготовить еженедельный отчёт",
            "expected_result": "Отчёт размещён в общем канале до вечера пятницы",
            "steps": ["Собрать метрики", "Согласовать с тимлидом"],
            "acceptance_criteria": ["Таблица обновлена"]
        }"#,
    )
    .expect("deserialises");

    assert!(draft.deadline_iso.is_none());
    assert!(!draft.refused);
    assert!(draft.refusal_reason.is_none());
    draft
        .validate_business_rules()
        .expect("non-refused draft should pass");
}

#[test]
fn given_iso_deadline_hint_when_deserialising_then_string_survives_roundtrip() {
    let draft: StructuredTaskDraft = serde_json::from_str(
        r#"{
            "title": "Подготовить статус",
            "expected_result": "Статус опубликован",
            "steps": ["Собрать черновик"],
            "acceptance_criteria": [],
            "deadline_iso": "2026-04-24T18:00:00+03:00"
        }"#,
    )
    .expect("deserialises");

    assert_eq!(
        draft.deadline_iso.as_deref(),
        Some("2026-04-24T18:00:00+03:00")
    );
}
