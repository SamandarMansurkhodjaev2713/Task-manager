use std::sync::Arc;

use chrono::Utc;
use tempfile::{tempdir, TempDir};

use telegram_task_bot::application::ports::repositories::EmployeeRepository;
use telegram_task_bot::application::use_cases::assignee_resolution::{
    AssigneeResolution, AssigneeResolver,
};
use telegram_task_bot::domain::user::OnboardingState;
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::{
    SqliteEmployeeRepository, SqliteUserRepository,
};

#[tokio::test]
async fn given_linked_user_without_username_when_resolving_by_employee_name_then_returns_direct_user(
) {
    let (_temp_dir, pool) = test_pool().await;
    let employee_repo = Arc::new(SqliteEmployeeRepository::new(pool.clone()));
    let user_repo = Arc::new(SqliteUserRepository::new(pool.clone()));

    let employee = employee_repo
        .upsert_bot_registered("Алина Смирнова", None, Utc::now())
        .await
        .expect("employee should be created");
    let employee_id = employee.id.expect("employee should have id");

    sqlx::query(
        "INSERT INTO users
             (telegram_id, last_chat_id, full_name, first_name, last_name,
              linked_employee_id, is_employee, onboarding_state, onboarding_version)
         VALUES (?, ?, ?, ?, ?, ?, 1, ?, 1)",
    )
    .bind(4242_i64)
    .bind(777_i64)
    .bind("Алина Смирнова")
    .bind("Алина")
    .bind("Смирнова")
    .bind(employee_id)
    .bind(OnboardingState::Completed.as_storage_value())
    .execute(&pool)
    .await
    .expect("linked user should be seeded");

    let resolver = AssigneeResolver::new(user_repo, employee_repo);
    let result = resolver
        .resolve_for_creation("Алина Смирнова")
        .await
        .expect("resolution should succeed");

    let AssigneeResolution::Resolved(resolved) = result else {
        panic!("expected direct resolution for unique employee name");
    };

    let user = resolved.user.expect("linked user should be returned");
    assert_eq!(user.telegram_id, 4242);
    assert_eq!(user.last_chat_id, Some(777));
    assert_eq!(user.linked_employee_id, Some(employee_id));
    assert_eq!(
        resolved.employee.and_then(|value| value.id),
        Some(employee_id)
    );
}

async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
    let temp_dir = tempdir().expect("temp dir should be created");
    let db_path = temp_dir.path().join("assignee-linked-user.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");
    (temp_dir, pool)
}
