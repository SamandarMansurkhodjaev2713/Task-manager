use crate::application::dto::task_views::{DeliveryStatus, TaskActionView};
use crate::presentation::telegram::callbacks::TaskListOrigin;

pub(crate) const MENU_EMOJI: &str = "🏠";
pub(crate) const CREATE_EMOJI: &str = "🆕";
pub(crate) const QUICK_EMOJI: &str = "⚡";
pub(crate) const GUIDED_EMOJI: &str = "🧭";
pub(crate) const TASKS_EMOJI: &str = "📋";
pub(crate) const SETTINGS_EMOJI: &str = "⚙️";
pub(crate) const HELP_EMOJI: &str = "❓";
pub(crate) const SYNC_EMOJI: &str = "🔄";
pub(crate) const DONE_EMOJI: &str = "✅";
pub(crate) const CANCEL_EMOJI: &str = "⛔";
pub(crate) const INFO_EMOJI: &str = "ℹ️";
pub(crate) const TIME_EMOJI: &str = "⏰";

const MAX_TASK_BUTTON_TITLE_LENGTH: usize = 24;

pub(crate) fn action_label(action: TaskActionView) -> &'static str {
    match action {
        TaskActionView::StartProgress => "▶️ В работу",
        TaskActionView::SubmitForReview => "🧪 На проверку",
        TaskActionView::ApproveReview => "✅ Принять",
        TaskActionView::ReturnToWork => "↩️ Вернуть",
        TaskActionView::Cancel => "⛔ Отменить",
        TaskActionView::ReportBlocker => "🚧 Есть блокер",
        TaskActionView::AddComment => "💬 Комментарий",
        TaskActionView::Reassign => "👤 Переназначить",
    }
}

pub(crate) fn back_label(origin: TaskListOrigin) -> &'static str {
    match origin {
        TaskListOrigin::Assigned => "↩️ К моим задачам",
        TaskListOrigin::Created => "↩️ К созданным мной",
        TaskListOrigin::Team => "↩️ К задачам команды",
    }
}

pub(crate) fn status_badge(status: &str) -> &'static str {
    match status {
        "created" => "🆕",
        "sent" => "📨",
        "in_progress" => "▶️",
        "blocked" => "🚧",
        "in_review" => "🧪",
        "completed" => "✅",
        "cancelled" => "⛔",
        _ => "ℹ️",
    }
}

pub(crate) fn status_label(status: &str) -> &'static str {
    match status {
        "created" => "создана",
        "sent" => "отправлена исполнителю",
        "in_progress" => "в работе",
        "blocked" => "есть блокер",
        "in_review" => "на проверке",
        "completed" => "завершена",
        "cancelled" => "отменена",
        _ => "неизвестно",
    }
}

pub(crate) fn delivery_badge(delivery_status: DeliveryStatus) -> &'static str {
    match delivery_status {
        DeliveryStatus::DeliveredToAssignee => "📬 Доставлено",
        DeliveryStatus::PendingDelivery => "🕓 В очереди",
        DeliveryStatus::PendingAssigneeRegistration => "👋 Ждёт /start",
        DeliveryStatus::RetryPending => "🔁 Повторим",
        DeliveryStatus::Failed => "⚠️ Не доставлено",
        DeliveryStatus::CreatorOnly => "👤 Только автору",
    }
}

pub(crate) fn truncate_title(value: &str) -> String {
    let truncated = value
        .chars()
        .take(MAX_TASK_BUTTON_TITLE_LENGTH)
        .collect::<String>();
    if value.chars().count() > MAX_TASK_BUTTON_TITLE_LENGTH {
        format!("{truncated}…")
    } else {
        truncated
    }
}

pub(crate) fn join_numbered_lines(values: &[String]) -> String {
    if values.is_empty() {
        return "1. Описание пока не указано".to_owned();
    }

    values
        .iter()
        .enumerate()
        .map(|(index, value)| format!("{}. {value}", index + 1))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn join_bullets(values: &[String]) -> String {
    if values.is_empty() {
        return "—".to_owned();
    }

    values
        .iter()
        .map(|value| format!("• {value}"))
        .collect::<Vec<_>>()
        .join("\n")
}
