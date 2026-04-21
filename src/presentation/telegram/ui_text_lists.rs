use crate::application::dto::task_views::{TaskCreationOutcome, TaskListPage};
use crate::presentation::telegram::callbacks::TaskListOrigin;
use crate::shared::constants::limits::MAX_TASK_CREATION_CONFIRM_PREVIEW_CHARS;

use super::super::ui_shared::{
    delivery_badge, delivery_detail, status_badge, status_label, INFO_EMOJI, TIME_EMOJI,
};

fn creation_confirm_body_preview(title: &str) -> String {
    let limit = MAX_TASK_CREATION_CONFIRM_PREVIEW_CHARS;
    let chars: usize = title.chars().count();
    if chars <= limit {
        return title.to_owned();
    }
    let short: String = title.chars().take(limit).collect();
    format!("{short}…\n\nПолный текст — в карточке задачи.")
}

pub fn task_creation_text(outcome: &TaskCreationOutcome) -> String {
    match outcome {
        TaskCreationOutcome::Created(summary) => format!(
            "✅ Задача создана\n\nКод: {}\nСтатус: {} {}\nДоставка: {} — {}\n\n{}\n\nОткройте карточку, чтобы продолжить работу.",
            summary.public_code,
            status_badge(summary.task.status),
            status_label(summary.task.status),
            delivery_badge(summary.delivery_status),
            delivery_detail(summary.delivery_status),
            creation_confirm_body_preview(&summary.task.title)
        ),
        TaskCreationOutcome::DuplicateFound(summary) => format!(
            "{INFO_EMOJI} Такая задача уже есть\n\nКод: {}\nСтатус: {} {}\n\nЯ не создавал дубль. Откройте текущую карточку и продолжайте работу из неё.",
            summary.public_code,
            status_badge(summary.task.status),
            status_label(summary.task.status),
        ),
        TaskCreationOutcome::ClarificationRequired(request) => {
            let task_anchor = request
                .task_body_preview
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|body| format!("📋 Текст задачи:\n{body}\n\n────────\n\n"))
                .unwrap_or_default();

            let candidates = if request.candidates.is_empty() {
                "Пока не вижу точного совпадения.".to_owned()
            } else {
                request
                    .candidates
                    .iter()
                    .map(|candidate| {
                        let username = candidate
                            .telegram_username
                            .as_ref()
                            .map(|value| format!(" (@{value})"))
                            .unwrap_or_default();
                        format!(
                            "• {}{} — {}%",
                            candidate.full_name, username, candidate.confidence
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            };

            format!(
                "{INFO_EMOJI} Нужно уточнить исполнителя\n\n{task_anchor}{}\n\n{candidates}",
                request.message
            )
        }
    }
}

pub fn list_header(origin: TaskListOrigin) -> (&'static str, &'static str) {
    match origin {
        TaskListOrigin::Assigned => ("📥 Мои задачи", "Здесь всё, что сейчас назначено вам."),
        TaskListOrigin::Created => (
            "📤 Созданные мной",
            "Задачи, которые вы поставили другим или себе.",
        ),
        TaskListOrigin::Team => (
            "👥 Задачи команды",
            "Общий список команды, чтобы видеть картину целиком.",
        ),
        TaskListOrigin::Focus => (
            "🧭 Мой фокус",
            "Экран внимания: что горит, ждёт вас или может застрять.",
        ),
        TaskListOrigin::ManagerInbox => (
            "🧪 Inbox менеджера",
            "Здесь собраны задачи, где чаще всего нужно ваше решение.",
        ),
    }
}

pub fn list_text(title: &str, subtitle: &str, page: &TaskListPage) -> String {
    if page.sections.is_empty() {
        return format!("{title}\n\n{subtitle}\n\nПока здесь пусто.");
    }

    let mut lines = vec![title.to_owned(), subtitle.to_owned(), String::new()];
    for section in &page.sections {
        lines.push(section.title.clone());
        lines.push(String::new());

        for task in &section.tasks {
            let deadline = task
                .deadline
                .map(|value| format!("{TIME_EMOJI} {}", value.format("%d.%m.%Y")))
                .unwrap_or_else(|| "Без срока".to_owned());
            let delivery = task
                .delivery_status
                .map(delivery_badge)
                .map(|value| format!(" • {value}"))
                .unwrap_or_default();
            let assignee = task
                .assigned_to_display
                .as_deref()
                .map(|value| format!(" • {value}"))
                .unwrap_or_default();
            let highlight = task
                .highlight
                .as_deref()
                .map(|value| format!("\n   ℹ️ {value}"))
                .unwrap_or_default();

            lines.push(format!(
                "• {} {} {}\n   {}{}{}{}",
                task.public_code,
                status_badge(task.status),
                task.title,
                deadline,
                assignee,
                delivery,
                highlight
            ));
        }

        lines.push(String::new());
    }

    lines.join("\n")
}
