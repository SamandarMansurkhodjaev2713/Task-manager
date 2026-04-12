use chrono::{NaiveDate, Utc};

use telegram_task_bot::domain::employee::Employee;
use telegram_task_bot::domain::task::{MessageType, StructuredTaskDraft, Task};

#[allow(dead_code)]
pub fn employee(full_name: &str, username: Option<&str>) -> Employee {
    let now = Utc::now();
    Employee {
        id: Some(1),
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

#[allow(dead_code)]
pub fn task(deadline: Option<NaiveDate>) -> Task {
    Task::new(
        "telegram:1:1".to_owned(),
        1,
        Some(2),
        Some(1),
        StructuredTaskDraft {
            title: "Подготовить релиз".to_owned(),
            expected_result: "Релиз опубликован без критических ошибок".to_owned(),
            steps: vec![
                "Проверить changelog".to_owned(),
                "Собрать артефакты".to_owned(),
                "Опубликовать релиз".to_owned(),
            ],
            acceptance_criteria: vec!["Smoke test проходит".to_owned()],
        },
        deadline,
        None,
        "Иван, нужно подготовить релиз".to_owned(),
        MessageType::Text,
        "test-model".to_owned(),
        "{}".to_owned(),
        1,
        1,
        Utc::now(),
    )
    .expect("factory task should be valid")
}
