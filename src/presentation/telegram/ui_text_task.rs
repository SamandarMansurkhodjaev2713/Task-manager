use crate::application::dto::task_views::{TaskCommentView, TaskStatusDetails};
use crate::domain::comment::CommentKind;
use crate::presentation::telegram::callbacks::TaskCardMode;

use super::super::ui_shared::{
    action_label, delivery_badge, join_bullets, join_numbered_lines, next_best_action,
    status_badge, status_label, TASKS_EMOJI,
};

const TASK_HISTORY_PREVIEW_LIMIT: usize = 4;
const COMPACT_DESCRIPTION_PREVIEW_LINES: usize = 2;

pub fn task_detail_text(details: &TaskStatusDetails, mode: TaskCardMode) -> String {
    match mode {
        TaskCardMode::Compact => compact_task_detail_text(details),
        TaskCardMode::Expanded => expanded_task_detail_text(details),
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
        "💬 Комментарий к задаче {} «{}»\n\nОтправьте короткий комментарий одним сообщением.\nОн появится в карточке и уйдёт участникам задачи.",
        details.public_code, details.title
    )
}

pub fn task_blocker_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "🚧 Блокер по задаче {} «{}»\n\nНапишите, что мешает двигаться дальше.\nЯ переведу задачу в статус блокера и уведомлю автора.",
        details.public_code, details.title
    )
}

pub fn task_reassign_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "👤 Переназначение задачи {} «{}»\n\nНапишите имя сотрудника или @username.\nЕсли совпадение будет неоднозначным, я попрошу уточнить.",
        details.public_code, details.title
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
    let delivery = details.delivery_status.map(delivery_badge).unwrap_or("—");
    let blocker = details
        .blocked_reason
        .as_deref()
        .map(|value| format!("\nБлокер: {value}"))
        .unwrap_or_default();
    let next_action = next_best_action(&details.available_actions)
        .map(action_label)
        .unwrap_or("дополнительных действий нет");
    let preview_steps = details
        .description_lines
        .iter()
        .take(COMPACT_DESCRIPTION_PREVIEW_LINES)
        .cloned()
        .collect::<Vec<_>>();
    let short_description = join_bullets(&preview_steps);

    format!(
        "{TASKS_EMOJI} {}\n\nКод: {}\nСтатус: {} {}\nСрок: {}\nИсполнитель: {}\nДоставка: {}\nСледующее действие: {}{}\n\nКоротко:\n{}",
        details.title,
        details.public_code,
        status_badge(details.status),
        status_label(details.status),
        deadline,
        assignee,
        delivery,
        next_action,
        blocker,
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
    let delivery = details.delivery_status.map(delivery_badge).unwrap_or("—");
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
    let blocker = details
        .blocked_reason
        .as_deref()
        .map(|value| format!("\nБлокер: {value}"))
        .unwrap_or_default();

    format!(
        "{TASKS_EMOJI} {}\n\nКод: {}\nСтатус: {} {}\nСрок: {}\nИсполнитель: {}\nДоставка: {}{}\n\nЧто нужно сделать:\n{}\n\nОжидаемый результат:\n{}\n\nКритерии приёмки:\n{}\n\nПоследние комментарии:\n{}\n\nПоследние изменения:\n{}",
        details.title,
        details.public_code,
        status_badge(details.status),
        status_label(details.status),
        deadline,
        assignee,
        delivery,
        blocker,
        description,
        details.expected_result,
        criteria,
        comments,
        history
    )
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
