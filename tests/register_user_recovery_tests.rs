use std::sync::Arc;

use tempfile::{tempdir, TempDir};

use telegram_task_bot::application::ports::repositories::{
    AuditLogRepository, NotificationRepository, TaskRepository,
};
use telegram_task_bot::application::use_cases::register_user::RegisterUserUseCase;
use telegram_task_bot::domain::audit::AuditAction;
use telegram_task_bot::domain::message::{IncomingMessage, MessageContent};
use telegram_task_bot::domain::notification::{NotificationDeliveryState, NotificationType};
use telegram_task_bot::domain::task::TaskStatus;
use telegram_task_bot::infrastructure::clock::system_clock::SystemClock;
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::{
    SqliteAuditLogRepository, SqliteEmployeeRepository, SqliteNotificationRepository,
    SqliteTaskRepository, SqliteUserRepository,
};

mod factories;

#[tokio::test]
async fn given_employee_assigned_task_without_user_when_employee_registers_then_links_task_and_enqueues_notification(
) {
    let (_temp_dir, pool) = test_pool().await;
    seed_creator_and_employee(&pool).await;

    let task_repository = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let notification_repository = Arc::new(SqliteNotificationRepository::new(pool.clone()));
    let audit_log_repository = Arc::new(SqliteAuditLogRepository::new(pool.clone()));
    let use_case = RegisterUserUseCase::new(
        Arc::new(SystemClock),
        Arc::new(SqliteUserRepository::new(pool.clone())),
        Arc::new(SqliteEmployeeRepository::new(pool.clone())),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    );

    let mut task = factories::task(None);
    task.assigned_to_user_id = None;
    task.assigned_to_employee_id = Some(1);
    task.status = TaskStatus::Created;
    let persisted_task = task_repository
        .create_if_absent(&task)
        .await
        .expect("task should be created");
    let task = match persisted_task {
        telegram_task_bot::application::ports::repositories::PersistedTask::Created(task) => task,
        telegram_task_bot::application::ports::repositories::PersistedTask::Existing(_) => {
            panic!("expected a newly created task")
        }
    };

    let actor = use_case
        .execute(&registration_message())
        .await
        .expect("user registration should succeed");
    let actor_id = actor.id.expect("registered user should have id");
    let task_id = task.id.expect("persisted task should have id");

    let linked_task = task_repository
        .find_by_uid(task.task_uid)
        .await
        .expect("task lookup should succeed")
        .expect("task should still exist");
    let notifications = notification_repository
        .find_latest_for_task_and_recipient(task_id, actor_id, NotificationType::TaskAssigned)
        .await
        .expect("notification lookup should succeed");
    let audit_entries = audit_log_repository
        .list_for_task(task_id)
        .await
        .expect("audit lookup should succeed");

    assert_eq!(linked_task.assigned_to_user_id, Some(actor_id));
    assert_eq!(linked_task.assigned_to_employee_id, Some(1));

    let notification = notifications.expect("assignment notification should exist");
    assert_eq!(
        notification.delivery_state,
        NotificationDeliveryState::Pending
    );

    assert!(audit_entries
        .iter()
        .any(|entry| matches!(entry.action, AuditAction::Assigned)));
}

#[tokio::test]
async fn given_same_employee_registers_twice_when_recovery_repeats_then_it_remains_idempotent() {
    let (_temp_dir, pool) = test_pool().await;
    seed_creator_and_employee(&pool).await;

    let task_repository = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let notification_repository = Arc::new(SqliteNotificationRepository::new(pool.clone()));
    let audit_log_repository = Arc::new(SqliteAuditLogRepository::new(pool.clone()));
    let use_case = RegisterUserUseCase::new(
        Arc::new(SystemClock),
        Arc::new(SqliteUserRepository::new(pool.clone())),
        Arc::new(SqliteEmployeeRepository::new(pool.clone())),
        task_repository.clone(),
        notification_repository.clone(),
        audit_log_repository.clone(),
    );

    let mut task = factories::task(None);
    task.assigned_to_user_id = None;
    task.assigned_to_employee_id = Some(1);
    let task = match task_repository
        .create_if_absent(&task)
        .await
        .expect("task should be created")
    {
        telegram_task_bot::application::ports::repositories::PersistedTask::Created(task) => task,
        telegram_task_bot::application::ports::repositories::PersistedTask::Existing(_) => {
            panic!("expected a newly created task")
        }
    };

    let first_actor = use_case
        .execute(&registration_message())
        .await
        .expect("first registration should succeed");
    let second_actor = use_case
        .execute(&registration_message())
        .await
        .expect("second registration should also succeed");
    let actor_id = first_actor.id.expect("first user should have id");
    let second_actor_id = second_actor.id.expect("second user should have id");
    let task_id = task.id.expect("task should have id");

    let notification = notification_repository
        .find_latest_for_task_and_recipient(task_id, actor_id, NotificationType::TaskAssigned)
        .await
        .expect("notification lookup should succeed")
        .expect("notification should exist");
    let audit_entries = audit_log_repository
        .list_for_task(task_id)
        .await
        .expect("audit entries should load");
    let linked_task = task_repository
        .find_by_uid(task.task_uid)
        .await
        .expect("task lookup should succeed")
        .expect("task should exist");

    assert_eq!(actor_id, second_actor_id);
    assert_eq!(linked_task.assigned_to_user_id, Some(actor_id));
    assert_eq!(
        notification.delivery_state,
        NotificationDeliveryState::Pending
    );
    assert_eq!(
        audit_entries
            .iter()
            .filter(|entry| matches!(entry.action, AuditAction::Assigned))
            .count(),
        1
    );
}

async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
    let temp_dir = tempdir().expect("temp dir should be created");
    let db_path = temp_dir.path().join("register-user.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");
    (temp_dir, pool)
}

async fn seed_creator_and_employee(pool: &sqlx::SqlitePool) {
    sqlx::query("INSERT INTO users (id, telegram_id, telegram_username, full_name) VALUES (1, 999, 'creator', 'Creator User')")
        .execute(pool)
        .await
        .expect("creator user should be seeded");
    sqlx::query("INSERT INTO employees (id, full_name, telegram_username) VALUES (1, 'Jean Dupont', 'jean_dupont')")
        .execute(pool)
        .await
        .expect("employee should be seeded");
}

fn registration_message() -> IncomingMessage {
    IncomingMessage {
        chat_id: 777,
        message_id: 1,
        sender_id: 4242,
        sender_name: "Jean Dupont".to_owned(),
        sender_username: Some("jean_dupont".to_owned()),
        content: MessageContent::Command {
            text: "/start".to_owned(),
        },
        timestamp: chrono::Utc::now(),
        source_message_key_override: None,
    }
}
