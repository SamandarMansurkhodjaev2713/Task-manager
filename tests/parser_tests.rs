mod factories;

use chrono::NaiveDate;

use telegram_task_bot::domain::parsing::parse_task_request;

#[test]
fn given_message_with_assignee_and_tomorrow_when_parse_then_extracts_fields() {
    let today = NaiveDate::from_ymd_opt(2026, 4, 10).expect("valid date");
    let parsed = parse_task_request("Иван, нужно подготовить релиз до завтра", today)
        .expect("parser should succeed");

    assert_eq!(parsed.assignee_name.as_deref(), Some("Иван"));
    assert_eq!(
        parsed.deadline,
        Some(NaiveDate::from_ymd_opt(2026, 4, 11).expect("valid date"))
    );
    assert!(parsed.task_description.contains("подготовить релиз"));
}

#[test]
fn given_short_message_when_parse_then_returns_validation_error() {
    let today = NaiveDate::from_ymd_opt(2026, 4, 10).expect("valid date");
    let result = parse_task_request("Иван, ок", today);

    assert!(result.is_err());
}

#[test]
fn given_message_with_username_assignee_when_parse_then_extracts_username() {
    let today = NaiveDate::from_ymd_opt(2026, 4, 10).expect("valid date");
    let parsed = parse_task_request("@ivan_petrov, подготовить отчёт до завтра", today)
        .expect("parser should succeed");

    assert_eq!(parsed.assignee_name.as_deref(), Some("ivan_petrov"));
}
