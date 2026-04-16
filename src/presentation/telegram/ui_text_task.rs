use crate::application::dto::task_views::DeliveryStatus;
use crate::application::dto::task_views::{TaskCommentView, TaskStatusDetails};
use crate::domain::comment::CommentKind;
use crate::presentation::telegram::callbacks::TaskCardMode;

use super::super::ui_shared::{
    action_label, delivery_badge, delivery_detail, join_bullets, join_numbered_lines,
    next_best_action, status_badge, status_label, TASKS_EMOJI,
};

const TASK_HISTORY_PREVIEW_LIMIT: usize = 4;
const COMPACT_DESCRIPTION_PREVIEW_LINES: usize = 2;

pub fn task_detail_text(
    details: &TaskStatusDetails,
    mode: TaskCardMode,
    notice: Option<&str>,
) -> String {
    let body = match mode {
        TaskCardMode::Compact => compact_task_detail_text(details),
        TaskCardMode::Expanded => expanded_task_detail_text(details),
    };

    match notice.filter(|value| !value.trim().is_empty()) {
        Some(notice) => format!("{notice}\n\n{body}"),
        None => body,
    }
}

pub fn cancel_confirmation_text(details: &TaskStatusDetails) -> String {
    format!(
        "⛔ Подтвердите отмену\n\nВы действительно хотите отменить задачу {} «{}»?\nЭто действие увидят участники задачи.",
        details.public_code, details.title
    )
}

pub fn task_comment_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "💬 Комментарий к задаче {} «{}»\n\nОтправьте один короткий комментарий. Он появится в карточке и уйдёт участникам задачи.",
        details.public_code, details.title
    )
}

pub fn task_blocker_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "🚧 Что мешает по задаче {} «{}»?\n\nОпишите блокер одним сообщением. Я отмечу задачу как заблокированную и покажу это автору и менеджеру.",
        details.public_code, details.title
    )
}

pub fn task_reassign_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "👤 Кому передать задачу {} «{}»?\n\nНапишите имя сотрудника или @username. Если совпадение будет неочевидным, я попрошу уточнить.",
        details.public_code, details.title
    )
}

pub fn delivery_help_text(details: &TaskStatusDetails) -> String {
    let assignee = details
        .assignee_display
        .clone()
        .unwrap_or_else(|| "исполнитель".to_owned());

    format!(
        "👋 Как подключить исполнителя\n\nЗадача {} уже сохранена, но {assignee} ещё не запускал бота.\n\nЧто делать дальше:\n• попросите сотрудника открыть бота\n• попросите его отправить /start\n• после этого задача начнёт доставляться напрямую автоматически\n\nТекст, который можно переслать:\n\n«Открой, пожалуйста, task bot и отправь команду /start. После этого я смогу назначать тебе задачи прямо в Telegram.»",
        details.public_code
    )
}

fn compact_task_detail_text(details: &TaskStatusDetails) -> String {
    let deadline = details
        .deadline
        .clone()
        .unwrap_or_else(|| "без срока".to_owned());
    let assignee = details
        .assignee_display
        .clone()
        .unwrap_or_else(|| "не указан".to_owned());
    let delivery = details.delivery_status;
    let delivery_line = render_compact_delivery(delivery);
    let priority_note = render_priority_note(details, delivery);
    let next_action = next_best_action(&details.available_actions)
        .map(action_label)
        .unwrap_or("доступных действий нет");
    let preview_steps = details
        .description_lines
        .iter()
        .take(COMPACT_DESCRIPTION_PREVIEW_LINES)
        .cloned()
        .collect::<Vec<_>>();
    let short_description = join_bullets(&preview_steps);

    format!(
        "{TASKS_EMOJI} {}\n\n{} • {} {}\nСрок: {}\nИсполнитель: {}\n{}\n{}\nСледующий шаг: {}\n\nКоротко:\n{}",
        details.title,
        details.public_code,
        status_badge(details.status),
        status_label(details.status),
        deadline,
        assignee,
        delivery_line,
        priority_note,
        next_action,
        short_description
    )
}

fn expanded_task_detail_text(details: &TaskStatusDetails) -> String {
    let deadline = details
        .deadline
        .clone()
        .unwrap_or_else(|| "без срока".to_owned());
    let assignee = details
        .assignee_display
        .clone()
        .unwrap_or_else(|| "не указан".to_owned());
    let delivery = details.delivery_status;
    let delivery_line = render_compact_delivery(delivery);
    let description = join_numbered_lines(&details.description_lines);
    let criteria = join_bullets(&details.acceptance_criteria);
    let history_preview = details
        .history_entries
        .iter()
        .take(TASK_HISTORY_PREVIEW_LIMIT)
        .cloned()
        .collect::<Vec<_>>();
    let history = join_bullets(&history_preview);
    let comments = render_comments(&details.comments);
    let priority_note = render_priority_note(details, delivery);
    let next_action = next_best_action(&details.available_actions)
        .map(action_label)
        .unwrap_or("доступных действий нет");

    format!(
        "{TASKS_EMOJI} {}\n\n{} • {} {}\nСрок: {}\nИсполнитель: {}\n{}\n{}\nСледующий шаг: {}\n\nЧто нужно сделать:\n{}\n\nОжидаемый результат:\n{}\n\nКритерии приёмки:\n{}\n\nПоследние комментарии:\n{}\n\nПоследние изменения:\n{}",
        details.title,
        details.public_code,
        status_badge(details.status),
        status_label(details.status),
        deadline,
        assignee,
        delivery_line,
        priority_note,
        next_action,
        description,
        details.expected_result,
        criteria,
        comments,
        history
    )
}

fn render_compact_delivery(delivery_status: Option<DeliveryStatus>) -> String {
    match delivery_status {
        Some(status) => format!(
            "Доставка: {} — {}",
            delivery_badge(status),
            delivery_detail(status)
        ),
        None => "Доставка: —".to_owned(),
    }
}

fn render_priority_note(
    details: &TaskStatusDetails,
    delivery_status: Option<DeliveryStatus>,
) -> String {
    if let Some(reason) = details.blocked_reason.as_deref() {
        return format!("Сейчас важно: есть блокер — {reason}");
    }

    if matches!(
        delivery_status,
        Some(DeliveryStatus::PendingAssigneeRegistration)
    ) {
        return "Сейчас важно: исполнитель ещё не открыл бота".to_owned();
    }

    if details.status == crate::domain::task::TaskStatus::InReview {
        return "Сейчас важно: задача ждёт решения по проверке".to_owned();
    }

    if details.deadline.is_some() {
        return "Сейчас важно: держите в фокусе срок и следующий шаг".to_owned();
    }

    "Сейчас важно: задача активна, можно продолжать работу".to_owned()
}

fn render_comments(comments: &[TaskCommentView]) -> String {
    if comments.is_empty() {
        return "—".to_owned();
    }

    comments
        .iter()
        .map(|comment| {
            let label = match comment.kind {
                CommentKind::Context => "контекст",
                CommentKind::Blocker => "блокер",
                CommentKind::System => "системное",
            };
            format!("• {} • {} • {}", comment.created_at, label, comment.body)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::task_detail_text;
    use crate::application::dto::task_views::TaskStatusDetails;
    use crate::domain::task::TaskStatus;
    use crate::presentation::telegram::callbacks::TaskCardMode;
    use uuid::Uuid;

    #[test]
    fn given_notice_when_rendering_task_detail_then_notice_is_prepended() {
        let details = build_task_status_details();

        let rendered =
            task_detail_text(&details, TaskCardMode::Compact, Some("✅ Статус обновлён"));

        assert!(rendered.starts_with("✅ Статус обновлён\n\n"));
        assert!(rendered.contains(&details.public_code));
    }

    #[test]
    fn given_no_notice_when_rendering_task_detail_then_card_starts_with_title_block() {
        let details = build_task_status_details();

        let rendered = task_detail_text(&details, TaskCardMode::Compact, None);

        assert!(rendered.starts_with("📋 Подготовить релиз"));
    }

    #[test]
    fn given_pending_registration_delivery_when_rendering_compact_card_then_explains_next_step() {
        let mut details = build_task_status_details();
        details.delivery_status =
            Some(crate::application::dto::task_views::DeliveryStatus::PendingAssigneeRegistration);

        let rendered = task_detail_text(&details, TaskCardMode::Compact, None);

        assert!(rendered.contains("Ждёт /start"));
        assert!(rendered.contains("исполнитель ещё не запускал бота"));
    }

    fn build_task_status_details() -> TaskStatusDetails {
        TaskStatusDetails {
            task_uid: Uuid::now_v7(),
            public_code: "T-0042".to_owned(),
            title: "Подготовить релиз".to_owned(),
            status: TaskStatus::InProgress,
            deadline: Some("16.04.2026".to_owned()),
            expected_result: "Релиз готов".to_owned(),
            description_lines: vec!["Собрать все изменения".to_owned()],
            acceptance_criteria: vec!["Все проверки пройдены".to_owned()],
            history_entries: vec!["15.04.2026 10:00: created -> in_progress".to_owned()],
            assignee_display: Some("@ivanov".to_owned()),
            delivery_status: None,
            blocked_reason: None,
            comments: Vec::new(),
            available_actions: Vec::new(),
        }
    }
}
