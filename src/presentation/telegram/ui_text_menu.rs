use crate::application::dto::task_views::StatsView;
use crate::domain::user::User;
use crate::presentation::telegram::drafts::GuidedTaskDraft;

use super::super::ui_shared::{
    CREATE_EMOJI, GUIDED_EMOJI, HELP_EMOJI, INFO_EMOJI, MENU_EMOJI, QUICK_EMOJI, SETTINGS_EMOJI,
    SYNC_EMOJI,
};

pub fn welcome_text(actor: &User) -> String {
    let role_hint = if actor.role.is_manager_or_admin() {
        "У вас открыт расширенный раздел команды: можно быстро смотреть review, блокеры и задачи, где нужно решение менеджера."
    } else {
        "Здесь удобно ставить задачи, держать фокус и быстро двигать работу дальше."
    };

    format!(
        "✨ Добро пожаловать, {}!\n\nЯ помогу поставить задачу, не потерять дедлайн и быстро понять, что требует внимания.\n{role_hint}\n\nВыберите, с чего начать:",
        actor.full_name.as_deref().unwrap_or("коллега")
    )
}

pub fn help_text() -> String {
    format!(
        "{HELP_EMOJI} Как пользоваться ботом\n\n1. Откройте «🧭 Мой фокус», если хотите быстро понять, что важно прямо сейчас.\n2. Нажмите «{CREATE_EMOJI} Создать задачу», если нужно поставить новую задачу.\n3. Работайте из карточки задачи: статус, комментарий, блокер и переназначение доступны в пару нажатий.\n\nПолезные команды:\n/start — главное меню\n/menu — вернуться в меню\n/new_task <текст> — быстро создать задачу\n/status <T-0001> — открыть задачу по коду\n/cancel_task <T-0001> — отменить задачу\n/my_tasks — мои задачи\n/created_tasks — созданные мной\n/team_tasks — задачи команды\n/stats — моя статистика\n/team_stats — статистика команды\n/settings — профиль и уведомления"
    )
}

pub fn create_menu_text() -> String {
    format!(
        "{CREATE_EMOJI} Создание задачи\n\n{QUICK_EMOJI} Быстрый режим — если задача уже сформулирована и её нужно просто отправить.\n{GUIDED_EMOJI} Пошаговый режим — если важно спокойно собрать исполнителя, описание и срок без ошибок."
    )
}

pub fn quick_create_prompt() -> String {
    format!(
        "{QUICK_EMOJI} Отправьте текст или голосовое сообщение с задачей.\n\nЕсли отправите голосовое, я сначала покажу короткую расшифровку и попрошу подтвердить её.\n\nПример:\n@ivanov подготовить релиз до пятницы\n\nКогда захотите выйти, нажмите «{MENU_EMOJI} В меню»."
    )
}

pub fn guided_assignee_prompt() -> String {
    format!(
        "{GUIDED_EMOJI} Шаг 1 из 3\n\nКого назначаем?\nМожно написать имя, фамилию или @username.\nЕсли задача пока без исполнителя, выберите кнопку ниже."
    )
}

pub fn guided_description_prompt() -> String {
    format!(
        "{GUIDED_EMOJI} Шаг 2 из 3\n\nКоротко и по делу опишите задачу.\nОдна сильная формулировка лучше длинного канцелярита.\n\nХорошо: «подготовить релизный чек-лист и сверить версии»\nСлабо: «сделать это»"
    )
}

pub fn guided_deadline_prompt() -> String {
    format!(
        "{GUIDED_EMOJI} Шаг 3 из 3\n\nУкажите срок в удобной форме:\n• завтра\n• пятница\n• 12.05\n• через 3 дня\n\nЕсли срока нет, нажмите «Без срока»."
    )
}

pub fn guided_confirmation_text(draft: &GuidedTaskDraft) -> String {
    let assignee = draft.assignee.as_deref().unwrap_or("без исполнителя");
    let description = draft.description.as_deref().unwrap_or("не указано");
    let deadline = draft.deadline.as_deref().unwrap_or("без срока");

    format!(
        "{INFO_EMOJI} Проверьте задачу перед созданием\n\nИсполнитель: {assignee}\nОписание: {description}\nСрок: {deadline}\n\nЕсли всё в порядке, подтвердите создание."
    )
}

pub fn voice_confirmation_text(transcript: &str) -> String {
    format!(
        "{INFO_EMOJI} Я разобрал голосовое так\n\n{transcript}\n\nЕсли всё верно, создайте задачу. Если нужно, сначала поправьте текст."
    )
}

pub fn voice_edit_prompt(transcript: &str) -> String {
    format!(
        "{GUIDED_EMOJI} Исправьте текст задачи\n\nСейчас у меня такая версия:\n{transcript}\n\nПришлите одним сообщением финальный текст задачи. Я заменю расшифровку и снова покажу подтверждение."
    )
}

pub fn stats_text(title: &str, stats: &StatsView) -> String {
    let average = stats
        .average_completion_hours
        .map(|value| format!("{value} ч"))
        .unwrap_or_else(|| "нет данных".to_owned());

    format!(
        "{title}\n\nВсего создано: {}\nАктивных: {}\nЗавершено: {}\nПросрочено: {}\nСреднее время выполнения: {}",
        stats.created_count,
        stats.active_count,
        stats.completed_count,
        stats.overdue_count,
        average
    )
}

pub fn settings_text(actor: &User) -> String {
    let notifications = if actor.last_chat_id.is_some() {
        "подключены"
    } else {
        "не подключены"
    };

    format!(
        "{SETTINGS_EMOJI} Профиль\n\nИмя: {}\nUsername: {}\nTelegram ID: {}\nРоль: {}\nУведомления: {}",
        actor.full_name.as_deref().unwrap_or("не указано"),
        actor.telegram_username.as_deref().unwrap_or("не указано"),
        actor.telegram_id,
        actor.role,
        notifications
    )
}

pub fn synced_text(count: usize) -> String {
    format!("{SYNC_EMOJI} Синхронизация завершена. Обновлено сотрудников: {count}.")
}
