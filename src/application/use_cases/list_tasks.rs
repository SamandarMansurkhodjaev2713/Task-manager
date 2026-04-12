use std::sync::Arc;

use chrono::NaiveDate;

use crate::application::dto::task_views::{TaskListItem, TaskListPage, TaskListSection};
use crate::application::ports::repositories::TaskRepository;
use crate::application::ports::services::Clock;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::task::{Task, TaskStatus};
use crate::domain::user::User;
use crate::shared::constants::limits::MANAGER_INBOX_STALE_DAYS;
use crate::shared::constants::pagination::{DEFAULT_PAGE_SIZE, MAX_PAGE_SIZE};

pub enum TaskListScope {
    AssignedToMe,
    CreatedByMe,
    Team,
    Focus,
    ManagerInbox,
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
            TaskListScope::AssignedToMe | TaskListScope::Focus => {
                self.task_repository
                    .list_assigned_to_user(actor_id, cursor, page_size)
                    .await?
            }
            TaskListScope::CreatedByMe => {
                self.task_repository
                    .list_created_by_user(actor_id, cursor, page_size)
                    .await?
            }
            TaskListScope::Team | TaskListScope::ManagerInbox => {
                ensure_team_access(actor)?;
                self.task_repository.list_all(cursor, page_size).await?
            }
        };

        let today = self.clock.today_utc();
        let next_cursor = tasks.last().map(|task| task.task_uid.to_string());
        Ok(TaskListPage {
            sections: build_sections(scope, today, &tasks),
            next_cursor,
        })
    }
}

fn ensure_team_access(actor: &User) -> AppResult<()> {
    if actor.role.is_manager_or_admin() {
        return Ok(());
    }

    Err(AppError::unauthorized(
        "Only managers and admins can view team task dashboards",
        serde_json::json!({ "telegram_id": actor.telegram_id }),
    ))
}

fn sanitize_limit(limit: Option<u32>) -> u32 {
    limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE)
}

fn build_sections(scope: TaskListScope, today: NaiveDate, tasks: &[Task]) -> Vec<TaskListSection> {
    match scope {
        TaskListScope::AssignedToMe => build_standard_sections(
            tasks,
            today,
            &[
                ("🔥 Просрочено", section_overdue),
                ("🧪 Ждёт проверки", section_in_review),
                ("🚧 Есть блокеры", section_blocked),
                ("⏰ На сегодня", section_due_today),
                ("📥 Новые", section_new),
                ("▶️ В работе", section_in_progress),
                ("✅ Завершено", section_completed),
                ("⛔ Отменено", section_cancelled),
            ],
        ),
        TaskListScope::CreatedByMe | TaskListScope::Team => build_standard_sections(
            tasks,
            today,
            &[
                ("⚠️ Требует внимания", section_attention),
                ("🧪 На проверке", section_in_review),
                ("🚧 Есть блокеры", section_blocked),
                ("▶️ В работе", section_in_progress),
                ("✅ Завершено", section_completed),
                ("⛔ Отменено", section_cancelled),
            ],
        ),
        TaskListScope::Focus => build_standard_sections(
            tasks,
            today,
            &[
                ("🔥 Горит сейчас", section_focus_urgent),
                ("🧭 Ждёт моего действия", section_focus_waiting_for_me),
                ("🚧 Заблокировано", section_blocked),
                ("🧪 На проверке", section_in_review),
                ("▶️ Остальное в работе", section_in_progress),
            ],
        ),
        TaskListScope::ManagerInbox => build_standard_sections(
            tasks,
            today,
            &[
                ("🧪 Ждёт решения менеджера", section_in_review),
                ("🚧 Нужна помощь с блокером", section_blocked),
                (
                    "👋 Исполнитель ещё не подключил бота",
                    section_pending_registration,
                ),
                ("🔥 Риск по срокам", section_deadline_risk),
                ("🕰️ Без движения", section_stale),
            ],
        ),
    }
}

type SectionPredicate = fn(&Task, NaiveDate) -> bool;

fn build_standard_sections(
    tasks: &[Task],
    today: NaiveDate,
    descriptors: &[(&str, SectionPredicate)],
) -> Vec<TaskListSection> {
    descriptors
        .iter()
        .filter_map(|(title, predicate)| {
            let items = tasks
                .iter()
                .filter(|task| predicate(task, today))
                .map(|task| TaskListItem::from_task(task, None, None, task_highlight(task, today)))
                .collect::<Vec<_>>();
            if items.is_empty() {
                return None;
            }

            Some(TaskListSection {
                title: (*title).to_owned(),
                tasks: items,
            })
        })
        .collect()
}

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
        || section_pending_registration(task, today)
}

fn section_focus_urgent(task: &Task, today: NaiveDate) -> bool {
    section_overdue(task, today) || section_due_today(task, today)
}

fn section_focus_waiting_for_me(task: &Task, _: NaiveDate) -> bool {
    matches!(task.status, TaskStatus::Created | TaskStatus::Sent)
}

fn section_pending_registration(task: &Task, _: NaiveDate) -> bool {
    task.assigned_to_employee_id.is_some() && task.assigned_to_user_id.is_none()
}

fn section_deadline_risk(task: &Task, today: NaiveDate) -> bool {
    if task.status.is_terminal() {
        return false;
    }

    task.deadline.is_some_and(|deadline| deadline <= today)
}

fn section_stale(task: &Task, today: NaiveDate) -> bool {
    if task.status.is_terminal() {
        return false;
    }

    let days_since_update = today
        .signed_duration_since(task.updated_at.date_naive())
        .num_days();
    days_since_update >= MANAGER_INBOX_STALE_DAYS
}

fn task_highlight(task: &Task, today: NaiveDate) -> Option<String> {
    if section_pending_registration(task, today) {
        return Some("нужен /start от исполнителя".to_owned());
    }

    if task.status == TaskStatus::Blocked {
        return task.blocked_reason.clone();
    }

    if section_overdue(task, today) {
        return Some("дедлайн уже прошёл".to_owned());
    }

    if section_due_today(task, today) {
        return Some("срок сегодня".to_owned());
    }

    if section_stale(task, today) {
        return Some("по задаче давно не было движения".to_owned());
    }

    None
}
