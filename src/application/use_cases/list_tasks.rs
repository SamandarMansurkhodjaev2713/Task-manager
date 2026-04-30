use std::sync::Arc;

use chrono::NaiveDate;

use crate::application::dto::task_views::{TaskListItem, TaskListPage, TaskListSection};
use crate::application::policies::role_authorization::RoleAuthorizationPolicy;
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

        // Fetch one extra row beyond the requested page so we can reliably
        // decide whether more data exists.  Without this, the last full page
        // would always display an "Ещё задачи" button that leads to an empty
        // screen — confusing and wasteful.
        let fetch_size = page_size.saturating_add(1);

        let mut tasks = match scope {
            TaskListScope::AssignedToMe | TaskListScope::Focus => {
                self.task_repository
                    .list_assigned_to_user(actor_id, cursor, fetch_size)
                    .await?
            }
            TaskListScope::CreatedByMe => {
                self.task_repository
                    .list_created_by_user(actor_id, cursor, fetch_size)
                    .await?
            }
            TaskListScope::Team => {
                RoleAuthorizationPolicy::ensure_can_view_team_tasks(actor)?;
                self.task_repository.list_all(cursor, fetch_size).await?
            }
            TaskListScope::ManagerInbox => {
                RoleAuthorizationPolicy::ensure_can_view_team_tasks(actor)?;
                // Manager inbox sections only inspect non-terminal tasks; fetch
                // only those to avoid pulling completed/cancelled rows into memory.
                self.task_repository.list_active(cursor, fetch_size).await?
            }
        };

        // If we got more than `page_size` rows the extra row is the sentinel
        // that proves additional pages exist.  Trim it before building sections
        // so only the correct page_size items are shown.
        let has_more = tasks.len() > page_size as usize;
        if has_more {
            tasks.truncate(page_size as usize);
        }

        let today = self.clock.today_utc();
        // next_cursor is the last *visible* task's UID.  The DB query uses
        // `task_uid < cursor ORDER BY task_uid DESC` so the subsequent page
        // starts immediately after this row.
        let next_cursor = if has_more {
            tasks.last().map(|task| task.task_uid.to_string())
        } else {
            None
        };
        Ok(TaskListPage {
            sections: build_sections(scope, today, &tasks),
            next_cursor,
        })
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn today() -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 4, 23).unwrap()
    }

    fn task_with_status(status: TaskStatus) -> Task {
        use crate::domain::task::{MessageType, StructuredTaskDraft};
        use chrono::Utc;
        let mut task = Task::new(
            "telegram:1:1".to_owned(),
            1,
            None,
            None,
            StructuredTaskDraft {
                title: "Тест".to_owned(),
                expected_result: "Результат".to_owned(),
                steps: vec!["Шаг 1".to_owned()],
                acceptance_criteria: vec!["Критерий 1".to_owned()],
                deadline_iso: None,
                refused: false,
                refusal_reason: None,
            },
            None,
            None,
            "test message".to_owned(),
            MessageType::Text,
            "test-model".to_owned(),
            "{}".to_owned(),
            1,
            1,
            Utc::now(),
        )
        .expect("factory task");
        task.status = status;
        task
    }

    /// Completed tasks must NOT appear in any manager inbox section.
    /// This validates the DB-level filter in `list_active` is semantically sound.
    #[test]
    fn given_completed_task_when_manager_inbox_sections_then_task_is_excluded_from_all_sections() {
        let today = today();
        let task = task_with_status(TaskStatus::Completed);

        assert!(
            !section_in_review(&task, today),
            "completed task must not appear in in_review"
        );
        assert!(
            !section_blocked(&task, today),
            "completed task must not appear in blocked"
        );
        assert!(
            !section_deadline_risk(&task, today),
            "completed task must not appear in deadline_risk"
        );
        assert!(
            !section_stale(&task, today),
            "completed task must not appear in stale"
        );
        assert!(
            !section_pending_registration(&task, today),
            "completed task must not appear in pending_registration"
        );
    }

    /// Cancelled tasks must NOT appear in any manager inbox section.
    #[test]
    fn given_cancelled_task_when_manager_inbox_sections_then_task_is_excluded_from_all_sections() {
        let today = today();
        let task = task_with_status(TaskStatus::Cancelled);

        assert!(!section_in_review(&task, today));
        assert!(!section_blocked(&task, today));
        assert!(!section_deadline_risk(&task, today));
        assert!(!section_stale(&task, today));
        assert!(!section_pending_registration(&task, today));
    }

    /// `InReview` tasks must appear in the `section_in_review` section.
    #[test]
    fn given_in_review_task_when_section_in_review_then_matches() {
        let task = task_with_status(TaskStatus::InReview);
        assert!(section_in_review(&task, today()));
    }

    /// `Blocked` tasks must appear in the `section_blocked` section.
    #[test]
    fn given_blocked_task_when_section_blocked_then_matches() {
        let task = task_with_status(TaskStatus::Blocked);
        assert!(section_blocked(&task, today()));
    }

    // ── Pagination sentinel tests ─────────────────────────────────────────

    /// When `has_more` is false (page is not full), `next_cursor` must be
    /// `None` so the "Ещё задачи" button is not shown.
    #[test]
    fn given_partial_page_when_build_page_then_no_next_cursor() {
        // Simulate: page_size=5, tasks returned = 3 (partial last page)
        let page_size: u32 = 5;
        let tasks: Vec<Task> = (0..3)
            .map(|_| task_with_status(TaskStatus::InProgress))
            .collect();

        let has_more = tasks.len() > page_size as usize;
        let next_cursor = if has_more {
            tasks.last().map(|t| t.task_uid.to_string())
        } else {
            None
        };

        assert!(!has_more, "partial page must not signal more data");
        assert!(next_cursor.is_none(), "no cursor on partial page");
    }

    /// When the repository returns exactly `page_size + 1` rows (sentinel),
    /// `has_more` is `true` and `next_cursor` is set to the last *visible*
    /// (page_size-th) row's UID.
    #[test]
    fn given_full_page_plus_sentinel_when_build_page_then_next_cursor_is_set() {
        let page_size: u32 = 5;
        // Simulate fetch_size = 6 rows returned; the 6th is the sentinel.
        let mut tasks: Vec<Task> = (0..6)
            .map(|_| task_with_status(TaskStatus::InProgress))
            .collect();

        let has_more = tasks.len() > page_size as usize;
        if has_more {
            tasks.truncate(page_size as usize);
        }
        let next_cursor = if has_more {
            tasks.last().map(|t| t.task_uid.to_string())
        } else {
            None
        };

        assert!(has_more, "sentinel row must signal more data");
        assert_eq!(
            tasks.len(),
            page_size as usize,
            "visible page must be trimmed to page_size"
        );
        assert!(
            next_cursor.is_some(),
            "cursor must be set when more data exists"
        );
    }
}
