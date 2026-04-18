use chrono::Utc;
use tempfile::tempdir;

use telegram_task_bot::application::ports::repositories::EmployeeRepository;
use telegram_task_bot::domain::employee::Employee;
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::SqliteEmployeeRepository;

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn employee(full_name: &str, username: Option<&str>) -> Employee {
    let now = Utc::now();
    Employee {
        id: None,
        full_name: full_name.to_owned(),
        telegram_username: username.map(ToOwned::to_owned),
        email: None,
        phone: None,
        department: None,
        is_active: true,
        synced_at: Some(now),
        created_at: now,
        updated_at: now,
    }
}

async fn setup_repo() -> SqliteEmployeeRepository {
    let temp_dir = tempdir().expect("temp dir");
    let db_path = temp_dir.path().join("employees_test.db");
    // leak the tempdir so the file outlives the test
    std::mem::forget(temp_dir);
    let url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&url).await.expect("pool");
    SqliteEmployeeRepository::new(pool)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

/// Two employees with the same full name but different usernames must both be
/// persisted and neither should overwrite the other (regression for C-2).
#[tokio::test]
async fn given_two_employees_with_same_name_when_upsert_then_both_are_stored() {
    let repo = setup_repo().await;

    let alice1 = employee("Иван Иванов", Some("ivanov1"));
    let alice2 = employee("Иван Иванов", Some("ivanov2"));

    repo.upsert_many(&[alice1, alice2])
        .await
        .expect("upsert should succeed");

    let active = repo.list_active().await.expect("list");
    let ivanov_rows: Vec<_> = active
        .iter()
        .filter(|e| e.full_name == "Иван Иванов")
        .collect();
    assert_eq!(
        ivanov_rows.len(),
        2,
        "both same-name employees must be stored; got {:?}",
        ivanov_rows
    );
}

/// Upserting the same employee by telegram_username twice must update rather
/// than insert a duplicate row.
#[tokio::test]
async fn given_employee_with_username_when_upserted_twice_then_single_row_updated() {
    let repo = setup_repo().await;

    let mut emp = employee("Мария Петрова", Some("petrova"));
    repo.upsert_many(&[emp.clone()])
        .await
        .expect("first upsert");

    emp.department = Some("Engineering".to_owned());
    repo.upsert_many(&[emp]).await.expect("second upsert");

    let active = repo.list_active().await.expect("list");
    let petrova: Vec<_> = active
        .iter()
        .filter(|e| e.telegram_username.as_deref() == Some("petrova"))
        .collect();
    assert_eq!(petrova.len(), 1, "must not create duplicate on re-upsert");
    assert_eq!(petrova[0].department.as_deref(), Some("Engineering"));
}

/// Employees without a username are matched by full_name among null-username
/// rows; only one row should exist after two upserts of the same name.
#[tokio::test]
async fn given_employee_without_username_when_upserted_twice_then_single_row_updated() {
    let repo = setup_repo().await;

    let mut emp = employee("Алексей Смирнов", None);
    repo.upsert_many(&[emp.clone()])
        .await
        .expect("first upsert");

    emp.department = Some("Operations".to_owned());
    repo.upsert_many(&[emp]).await.expect("second upsert");

    let active = repo.list_active().await.expect("list");
    let smirnov: Vec<_> = active
        .iter()
        .filter(|e| e.full_name == "Алексей Смирнов")
        .collect();
    assert_eq!(
        smirnov.len(),
        1,
        "must not create duplicate for no-username employee"
    );
    assert_eq!(smirnov[0].department.as_deref(), Some("Operations"));
}

/// Two no-username employees with the same name within a single sync batch are
/// treated as the same person (the second entry updates the first).
///
/// Rationale: without a Telegram username we have no stable identity key.
/// Treating them as the same record keeps the sync idempotent: repeated syncs
/// of the same source data always converge to a single row rather than growing
/// unboundedly.  The far rarer case of two genuinely different people sharing an
/// exact name AND neither having a username is a known limitation; the fix for
/// C-2 (distinct-username employees no longer clobbering each other) covers the
/// common real-world scenario.
#[tokio::test]
async fn given_two_no_username_employees_with_same_name_when_upsert_then_treated_as_same_record() {
    let repo = setup_repo().await;

    let mut e1 = employee("Сергей Козлов", None);
    e1.department = Some("Sales".to_owned());
    let mut e2 = employee("Сергей Козлов", None);
    e2.department = Some("Engineering".to_owned());

    repo.upsert_many(&[e1, e2]).await.expect("upsert");

    let active = repo.list_active().await.expect("list");
    let kozlov: Vec<_> = active
        .iter()
        .filter(|e| e.full_name == "Сергей Козлов")
        .collect();
    assert_eq!(
        kozlov.len(),
        1,
        "no-username same-name entries converge to one row to keep syncs idempotent"
    );
    // The second entry's department wins (last-write-wins within the batch).
    assert_eq!(kozlov[0].department.as_deref(), Some("Engineering"));
}
