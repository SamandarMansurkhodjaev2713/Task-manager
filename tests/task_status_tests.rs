mod factories;

use chrono::{NaiveDate, Utc};

use telegram_task_bot::domain::task::TaskStatus;

#[test]
fn given_created_assigned_task_when_transition_to_in_progress_then_succeeds() {
    let task = factories::task(Some(
        NaiveDate::from_ymd_opt(2026, 4, 12).expect("valid date"),
    ));

    let result = task
        .transition_to(TaskStatus::InProgress, Utc::now())
        .expect("created -> in_progress should succeed for directly opened assigned tasks");

    assert_eq!(result.status, TaskStatus::InProgress);
    assert!(result.started_at.is_some());
}

#[test]
fn given_sent_task_when_transition_to_in_review_then_succeeds() {
    let task = factories::task(None);
    let sent_task = task
        .transition_to(TaskStatus::Sent, Utc::now())
        .expect("created -> sent should succeed");

    let review_task = sent_task
        .transition_to(TaskStatus::InReview, Utc::now())
        .expect("sent -> in_review should succeed");

    assert_eq!(review_task.status, TaskStatus::InReview);
    assert!(review_task.review_requested_at.is_some());
}

#[test]
fn given_in_review_task_when_transition_to_completed_then_succeeds() {
    let task = factories::task(None);
    let review_task = task
        .transition_to(TaskStatus::Sent, Utc::now())
        .and_then(|task| task.transition_to(TaskStatus::InReview, Utc::now()))
        .expect("review transition should succeed");

    let completed_task = review_task
        .transition_to(TaskStatus::Completed, Utc::now())
        .expect("in_review -> completed should succeed");

    assert_eq!(completed_task.status, TaskStatus::Completed);
    assert!(completed_task.completed_at.is_some());
}
