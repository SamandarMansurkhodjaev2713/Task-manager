use std::sync::Arc;

use chrono::NaiveDate;

use crate::application::dto::task_views::{TaskListItem, TaskListPage, TaskListSection};
use crate::application::ports::repositories::TaskRepository;
use crate::application::ports::services::Clock;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::task::{Task, TaskStatus};
use crate::domain::user::User;
use crate::shared::constants::pagination::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};

pub enum TaskListScope {
    AssignedToMe,
    CreatedByMe,
    Team,
}

pub struct ListTasksUseCase {
    clock: Arc<dyn Clock>,
    task_repository: Arc<dyn TaskRepository>,
}

impl ListTasksUseCase {
    pub fn new(clock: Arc<dyn Clock>, task_repository: Arc<dyn TaskRepository>) -> Self {
        Self {
            clock,
            task_repository,
        }
    }

    pub async fn execute(
        &self,
        actor: &User,
        scope: TaskListScope,
        cursor: Option<String>,
        limit: Option<u32>,
    ) -> AppResult<TaskListPage> {
        let actor_id = actor.id.ok_or_else(|| {
            AppError::unauthenticated(
                "User must be registered before listing tasks",
                serde_json::json!({ "telegram_id": actor.telegram_id }),
            )
        })?;
        let page_size = sanitize_limit(limit);
        let tasks = match scope {
            TaskListScope::AssignedToMe => {
                self.task_repository
                    .list_assigned_to_user(actor_id, cursor, page_size)
                    .await?
            }
            TaskListScope::CreatedByMe => {
                self.task_repository
                    .list_created_by_user(actor_id, cursor, page_size)
                    .await?
            }
            TaskListScope::Team => {
                if !actor.role.is_manager_or_admin() {
                    return Err(AppError::unauthorized(
                        "Only managers and admins can view team tasks",
                        serde_json::json!({ "telegram_id": actor.telegram_id }),
                    ));
                }
                self.task_repository.list_all(cursor, page_size).await?
            }
        };

        let next_cursor = tasks.last().map(|task| task.task_uid.to_string());
        Ok(TaskListPage {
            sections: build_sections(scope, self.clock.today_utc(), &tasks),
            next_cursor,
        })
    }
}

fn sanitize_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE)
}

fn build_sections(scope: TaskListScope, today: NaiveDate, tasks: &[Task]) -> Vec<TaskListSection> {
    let descriptors = match scope {
        TaskListScope::AssignedToMe => vec![
            ("🔥 Просрочено", section_overdue as SectionPredicate),
            ("🧪 Ждёт проверки", section_in_review as SectionPredicate),
            ("🚧 Есть блокеры", section_blocked as SectionPredicate),
            ("⏰ На сегодня", section_due_today as SectionPredicate),
            ("📥 Новые", section_new as SectionPredicate),
            ("▶️ В работе", section_in_progress as SectionPredicate),
            ("✅ Завершено", section_completed as SectionPredicate),
            ("⛔ Отменено", section_cancelled as SectionPredicate),
        ],
        TaskListScope::CreatedByMe | TaskListScope::Team => vec![
            ("⚠️ Требует внимания", section_attention as SectionPredicate),
            ("🧪 На проверке", section_in_review as SectionPredicate),
            ("🚧 Есть блокеры", section_blocked as SectionPredicate),
            ("▶️ В работе", section_in_progress as SectionPredicate),
            ("✅ Завершено", section_completed as SectionPredicate),
            ("⛔ Отменено", section_cancelled as SectionPredicate),
        ],
    };

    descriptors
        .into_iter()
        .filter_map(|(title, predicate)| {
            let items = tasks
                .iter()
                .filter(|task| predicate(task, today))
                .map(|task| TaskListItem::from_task(task, None, None, task_highlight(task, today)))
                .collect::<Vec<_>>();
            if items.is_empty() {
                None
            } else {
                Some(TaskListSection {
                    title: title.to_owned(),
                    tasks: items,
                })
            }
        })
        .collect()
}

type SectionPredicate = fn(&Task, NaiveDate) -> bool;

fn section_overdue(task: &Task, today: NaiveDate) -> bool {
    task.deadline.is_some_and(|deadline| deadline < today) && !task.status.is_terminal()
}

fn section_due_today(task: &Task, today: NaiveDate) -> bool {
    task.deadline == Some(today) && !task.status.is_terminal() && task.status != TaskStatus::Blocked
}

fn section_in_review(task: &Task, _: NaiveDate) -> bool {
    task.status == TaskStatus::InReview
}

fn section_blocked(task: &Task, _: NaiveDate) -> bool {
    task.status == TaskStatus::Blocked
}

fn section_new(task: &Task, _: NaiveDate) -> bool {
    matches!(task.status, TaskStatus::Created | TaskStatus::Sent)
}

fn section_in_progress(task: &Task, _: NaiveDate) -> bool {
    task.status == TaskStatus::InProgress
}

fn section_completed(task: &Task, _: NaiveDate) -> bool {
    task.status == TaskStatus::Completed
}

fn section_cancelled(task: &Task, _: NaiveDate) -> bool {
    task.status == TaskStatus::Cancelled
}

fn section_attention(task: &Task, today: NaiveDate) -> bool {
    section_overdue(task, today)
        || task.status == TaskStatus::Blocked
        || matches!(task.status, TaskStatus::Created | TaskStatus::Sent)
        || (task.assigned_to_employee_id.is_some() && task.assigned_to_user_id.is_none())
}

fn task_highlight(task: &Task, today: NaiveDate) -> Option<String> {
    if task.assigned_to_employee_id.is_some() && task.assigned_to_user_id.is_none() {
        return Some("нужен /start у исполнителя".to_owned());
    }

    if task.status == TaskStatus::Blocked {
        return task.blocked_reason.clone();
    }

    if task.deadline.is_some_and(|deadline| deadline < today) && !task.status.is_terminal() {
        return Some("дедлайн уже прошёл".to_owned());
    }

    if task.deadline == Some(today) && !task.status.is_terminal() {
        return Some("срок сегодня".to_owned());
    }

    None
}
