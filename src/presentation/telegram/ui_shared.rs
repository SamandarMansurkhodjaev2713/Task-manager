use crate::application::dto::task_views::DeliveryStatus;
use crate::application::dto::task_views::TaskActionView;
use crate::domain::task::TaskStatus;
use crate::presentation::telegram::callbacks::TaskListOrigin;
use crate::shared::constants::limits::MAX_TASK_BUTTON_TITLE_LENGTH;

pub(crate) const MENU_EMOJI: &str = "🏠";
pub(crate) const CREATE_EMOJI: &str = "🆕";
pub(crate) const QUICK_EMOJI: &str = "⚡";
pub(crate) const GUIDED_EMOJI: &str = "🧭";
pub(crate) const TASKS_EMOJI: &str = "📋";
pub(crate) const SETTINGS_EMOJI: &str = "⚙️";
pub(crate) const HELP_EMOJI: &str = "❓";
pub(crate) const SYNC_EMOJI: &str = "🔄";
pub(crate) const INFO_EMOJI: &str = "ℹ️";
pub(crate) const TIME_EMOJI: &str = "⏰";

pub(crate) fn action_label(action: TaskActionView) -> &'static str {
    match action {
        TaskActionView::StartProgress => "▶️ Взять в работу",
        TaskActionView::SubmitForReview => "🧪 Отправить на проверку",
        TaskActionView::ApproveReview => "✅ Принять работу",
        TaskActionView::ReturnToWork => "↩️ Вернуть в работу",
        TaskActionView::Cancel => "⛔ Отменить",
        TaskActionView::ReportBlocker => "🚧 Сообщить о блокере",
        TaskActionView::AddComment => "💬 Комментарий",
        TaskActionView::Reassign => "👤 Переназначить",
    }
}

pub(crate) fn back_label(origin: TaskListOrigin) -> &'static str {
    match origin {
        TaskListOrigin::Assigned => "↩️ К моим задачам",
        TaskListOrigin::Created => "↩️ К созданным мной",
        TaskListOrigin::Team => "↩️ К задачам команды",
        TaskListOrigin::Focus => "↩️ К моему фокусу",
        TaskListOrigin::ManagerInbox => "↩️ К inbox менеджера",
    }
}

pub(crate) fn status_badge(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Created => "🆕",
        TaskStatus::Sent => "📨",
        TaskStatus::InProgress => "▶️",
        TaskStatus::Blocked => "🚧",
        TaskStatus::InReview => "🧪",
        TaskStatus::Completed => "✅",
        TaskStatus::Cancelled => "⛔",
    }
}

pub(crate) fn status_label(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Created => "новая",
        TaskStatus::Sent => "отправлена исполнителю",
        TaskStatus::InProgress => "в работе",
        TaskStatus::Blocked => "есть блокер",
        TaskStatus::InReview => "на проверке",
        TaskStatus::Completed => "завершена",
        TaskStatus::Cancelled => "отменена",
    }
}

pub(crate) fn delivery_badge(delivery_status: DeliveryStatus) -> &'static str {
    match delivery_status {
        DeliveryStatus::DeliveredToAssignee => "📬 Доставлено",
        DeliveryStatus::PendingDelivery => "🕓 В очереди",
        DeliveryStatus::PendingAssigneeRegistration => "👋 Ждёт /start",
        DeliveryStatus::RetryPending => "🔁 Повторим отправку",
        DeliveryStatus::Failed => "⚠️ Не доставлено",
        DeliveryStatus::CreatorOnly => "👤 Только автору",
    }
}

pub(crate) fn delivery_detail(delivery_status: DeliveryStatus) -> &'static str {
    match delivery_status {
        DeliveryStatus::DeliveredToAssignee => "исполнитель уже получил задачу в Telegram",
        DeliveryStatus::PendingDelivery => {
            "уведомление уже в очереди, бот доставит его автоматически"
        }
        DeliveryStatus::PendingAssigneeRegistration => {
            "исполнитель ещё не запускал бота; задача сохранена и придёт после /start"
        }
        DeliveryStatus::RetryPending => "прошлая отправка не удалась, бот повторит попытку",
        DeliveryStatus::Failed => {
            "доставить уведомление пока не получилось, нужно проверить подключение исполнителя"
        }
        DeliveryStatus::CreatorOnly => "задача создана без отдельного уведомления исполнителю",
    }
}

pub(crate) fn truncate_title(value: &str) -> String {
    let truncated = value
        .chars()
        .take(MAX_TASK_BUTTON_TITLE_LENGTH)
        .collect::<String>();
    if value.chars().count() > MAX_TASK_BUTTON_TITLE_LENGTH {
        return format!("{truncated}…");
    }

    truncated
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

pub(crate) fn next_best_action(actions: &[TaskActionView]) -> Option<TaskActionView> {
    const ACTION_PRIORITY: [TaskActionView; 5] = [
        TaskActionView::StartProgress,
        TaskActionView::SubmitForReview,
        TaskActionView::ApproveReview,
        TaskActionView::ReportBlocker,
        TaskActionView::Reassign,
    ];

    ACTION_PRIORITY
        .iter()
        .copied()
        .find(|candidate| actions.contains(candidate))
        .or_else(|| actions.first().copied())
}

pub(crate) fn is_dangerous_action(action: TaskActionView) -> bool {
    matches!(action, TaskActionView::Cancel)
}
