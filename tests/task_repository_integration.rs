use tempfile::tempdir;

use telegram_task_bot::application::ports::repositories::{PersistedTask, TaskRepository};
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::SqliteTaskRepository;

mod factories;

#[tokio::test]
async fn given_new_task_when_create_if_absent_then_persists_and_deduplicates() {
    let temp_dir = tempdir().expect("temp dir should be created");
    let db_path = temp_dir.path().join("integration.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");

    sqlx::query("INSERT INTO users (id, telegram_id) VALUES (1, 100), (2, 200)")
        .execute(&pool)
        .await
        .expect("seed users for foreign keys");
    sqlx::query("INSERT INTO employees (id, full_name) VALUES (1, 'Test Employee')")
        .execute(&pool)
        .await
        .expect("seed employee for foreign keys");

    let repository = SqliteTaskRepository::new(pool);
    let task = factories::task(None);

    let first_insert = repository
        .create_if_absent(&task)
        .await
        .expect("first insert should succeed");
    let second_insert = repository
        .create_if_absent(&task)
        .await
        .expect("second insert should succeed");

    assert!(matches!(first_insert, PersistedTask::Created(_)));
    assert!(matches!(second_insert, PersistedTask::Existing(_)));
}
