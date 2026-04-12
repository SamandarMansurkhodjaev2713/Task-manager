use crate::application::dto::task_views::{
    StatsView, TaskCommentView, TaskCreationOutcome, TaskListPage, TaskStatusDetails,
};
use crate::domain::comment::CommentKind;
use crate::domain::user::User;
use crate::presentation::telegram::callbacks::TaskListOrigin;
use crate::presentation::telegram::drafts::GuidedTaskDraft;
use crate::presentation::telegram::ui_shared::{
    delivery_badge, join_bullets, join_numbered_lines, status_badge, status_label, CREATE_EMOJI,
    DONE_EMOJI, GUIDED_EMOJI, HELP_EMOJI, INFO_EMOJI, MENU_EMOJI, QUICK_EMOJI, SETTINGS_EMOJI,
    SYNC_EMOJI, TASKS_EMOJI, TIME_EMOJI,
};

const TASK_HISTORY_PREVIEW_LIMIT: usize = 4;

pub fn welcome_text(actor: &User) -> String {
    let role_hint = if actor.role.is_manager_or_admin() {
        "У вас открыт расширенный раздел команды: можно смотреть общие задачи, блокеры и задачи на проверке."
    } else {
        "Здесь удобно ставить задачи, быстро открывать карточки и менять статусы без лишних команд."
    };

    format!(
        "✨ Добро пожаловать, {}!\n\nЯ помогу поставить задачу, не потерять дедлайн и быстро понять, что требует внимания.\n{role_hint}\n\nВыберите, что хотите сделать:",
        actor.full_name.as_deref().unwrap_or("коллега")
    )
}

pub fn help_text() -> String {
    format!(
        "{HELP_EMOJI} Как пользоваться ботом\n\n1. Нажмите «{CREATE_EMOJI} Создать задачу».\n2. Выберите быстрый режим или пошаговый мастер.\n3. Открывайте карточку задачи и работайте через кнопки: статус, комментарий, блокер, переназначение.\n\nПолезные команды:\n/start — открыть главное меню\n/menu — вернуться в меню\n/new_task <текст> — быстро создать задачу\n/my_tasks — мои задачи\n/created_tasks — созданные мной\n/team_tasks — задачи команды\n/stats — моя статистика\n/team_stats — статистика команды\n/settings — профиль и доставка уведомлений"
    )
}

pub fn create_menu_text() -> String {
    format!(
        "{CREATE_EMOJI} Создание задачи\n\n{QUICK_EMOJI} Быстрый режим — когда задача уже сформулирована и её нужно просто отправить.\n{GUIDED_EMOJI} Пошаговый режим — когда важно аккуратно указать исполнителя, описание и срок."
    )
}

pub fn quick_create_prompt() -> String {
    format!(
        "{QUICK_EMOJI} Отправьте текст или голосовое сообщение с задачей.\n\nПример:\n@ivanov подготовить релиз до пятницы\n\nКогда захотите выйти, нажмите «{MENU_EMOJI} В меню»."
    )
}

pub fn guided_assignee_prompt() -> String {
    format!(
        "{GUIDED_EMOJI} Шаг 1 из 3\n\nУкажите исполнителя.\nМожно написать имя, фамилию или @username.\nЕсли задача пока без исполнителя, выберите кнопку ниже."
    )
}

pub fn guided_description_prompt() -> String {
    format!(
        "{GUIDED_EMOJI} Шаг 2 из 3\n\nКоротко и понятно опишите задачу.\nОдна хорошая формулировка лучше длинного канцелярита."
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

pub fn task_creation_text(outcome: &TaskCreationOutcome) -> String {
    match outcome {
        TaskCreationOutcome::Created(summary) => format!(
            "{DONE_EMOJI} Готово\n\n{}\n\nОткройте карточку задачи или вернитесь в меню.",
            summary.message
        ),
        TaskCreationOutcome::ClarificationRequired(request) => {
            let candidates = if request.candidates.is_empty() {
                "Пока нет точного совпадения.".to_owned()
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
                "{INFO_EMOJI} Нужно чуть уточнить исполнителя\n\n{}\n\n{}",
                request.message, candidates
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
            "Общий список для контроля команды и узких мест.",
        ),
    }
}

pub fn list_text(title: &str, subtitle: &str, page: &TaskListPage) -> String {
    if page.sections.is_empty() {
        return format!("{title}\n\n{subtitle}\n\nПока задач нет.");
    }

    let mut lines = vec![title.to_owned(), subtitle.to_owned(), String::new()];
    for section in &page.sections {
        lines.push(section.title.clone());
        lines.push(String::new());

        for (index, task) in section.tasks.iter().enumerate() {
            let deadline = task
                .deadline
                .map(|value| format!("{TIME_EMOJI} {}", value.format("%d.%m.%Y")))
                .unwrap_or_else(|| "Без срока".to_owned());
            let assignee = task
                .assigned_to_display
                .as_deref()
                .map(|value| format!(" • {value}"))
                .unwrap_or_default();
            let delivery = task
                .delivery_status
                .map(delivery_badge)
                .map(|value| format!(" • {value}"))
                .unwrap_or_default();
            let highlight = task
                .highlight
                .as_deref()
                .map(|value| format!("\n   ℹ️ {value}"))
                .unwrap_or_default();

            lines.push(format!(
                "{}. {} {}\n{}{}{}{}",
                index + 1,
                status_badge(&task.status.to_string()),
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

pub fn task_detail_text(details: &TaskStatusDetails) -> String {
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
        "{TASKS_EMOJI} {}\n\nСтатус: {}\nСрок: {}\nИсполнитель: {}\nДоставка: {}{}\nID: {}\n\nЧто нужно сделать:\n{}\n\nОжидаемый результат:\n{}\n\nКритерии приёмки:\n{}\n\nПоследние комментарии:\n{}\n\nПоследние изменения:\n{}",
        details.title,
        format!("{} {}", status_badge(&details.status), status_label(&details.status)),
        deadline,
        assignee,
        delivery,
        blocker,
        details.task_uid,
        description,
        details.expected_result,
        criteria,
        comments,
        history
    )
}

pub fn cancel_confirmation_text(details: &TaskStatusDetails) -> String {
    format!(
        "⛔ Подтвердите отмену\n\nВы действительно хотите отменить задачу «{}»?\nЭто действие увидят участники задачи.",
        details.title
    )
}

pub fn task_comment_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "💬 Комментарий к задаче «{}»\n\nОтправьте короткий комментарий одним сообщением.\nОн появится в карточке и уйдёт участникам задачи.",
        details.title
    )
}

pub fn task_blocker_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "🚧 Блокер по задаче «{}»\n\nНапишите, что мешает продолжить работу.\nЯ переведу задачу в статус блокера и уведомлю автора.",
        details.title
    )
}

pub fn task_reassign_prompt(details: &TaskStatusDetails) -> String {
    format!(
        "👤 Переназначение задачи «{}»\n\nНапишите имя сотрудника или @username.\nЕсли совпадение будет неоднозначным, я попрошу уточнить.",
        details.title
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

fn render_comments(comments: &[TaskCommentView]) -> String {
    if comments.is_empty() {
        return "—".to_owned();
    }

    comments
        .iter()
        .map(|comment| {
            format!(
                "• {} {} — {}",
                comment_kind_label(comment.kind),
                comment.created_at,
                comment.body
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn comment_kind_label(kind: CommentKind) -> &'static str {
    match kind {
        CommentKind::Context => "Комментарий",
        CommentKind::Blocker => "Блокер",
        CommentKind::System => "Система",
    }
}
