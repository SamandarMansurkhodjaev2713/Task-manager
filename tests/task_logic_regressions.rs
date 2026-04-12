mod factories;

use chrono::NaiveDate;
use chrono::Utc;

use telegram_task_bot::domain::message::{IncomingMessage, MessageContent};
use telegram_task_bot::domain::parsing::parse_task_request;
use telegram_task_bot::domain::task::TaskStatus;

#[test]
fn given_message_with_username_without_comma_when_parse_then_assignee_is_extracted() {
    let today = NaiveDate::from_ymd_opt(2026, 4, 11).expect("valid date");
    let parsed = parse_task_request("@ivan_petrov prepare quarterly release notes", today)
        .expect("parser should succeed");

    assert_eq!(parsed.assignee_name.as_deref(), Some("ivan_petrov"));
    assert_eq!(parsed.task_description, "prepare quarterly release notes");
}

#[test]
fn given_cancelled_task_when_cancel_requested_again_then_transition_is_rejected() {
    let task = factories::task(None);
    let cancelled_task = task
        .transition_to(TaskStatus::Cancelled, Utc::now())
        .expect("initial cancel transition should succeed");

    let result = cancelled_task.transition_to(TaskStatus::Cancelled, Utc::now());

    assert!(result.is_err());
}

#[test]
fn given_source_message_key_override_when_requested_then_override_is_used() {
    let message = IncomingMessage {
        message_id: 0,
        chat_id: 42,
        sender_id: 7,
        sender_name: "tester".to_owned(),
        sender_username: Some("tester".to_owned()),
        content: MessageContent::Text {
            text: "prepare release".to_owned(),
        },
        timestamp: Utc::now(),
        source_message_key_override: Some("telegram:guided:42:draft-1".to_owned()),
    };

    assert_eq!(message.source_message_key(), "telegram:guided:42:draft-1");
}

#[test]
fn given_reassigned_task_when_assignment_changes_then_delivery_state_resets_to_created() {
    let task = factories::task(None);
    let sent_task = task
        .transition_to(TaskStatus::Sent, Utc::now())
        .expect("created -> sent should succeed");

    let reassigned_task = sent_task
        .reassign(Some(777), None, Utc::now())
        .expect("reassign should succeed");

    assert_eq!(reassigned_task.status, TaskStatus::Created);
    assert!(reassigned_task.sent_at.is_none());
}
