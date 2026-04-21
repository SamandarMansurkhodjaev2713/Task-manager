use crate::application::dto::task_views::{
    AssigneeInterpretation, EmployeeCandidateView, StatsView, TaskInterpretationPreview,
};
use crate::domain::user::User;
use crate::presentation::telegram::drafts::GuidedTaskDraft;

use super::super::ui_shared::{
    delivery_badge, delivery_detail, CREATE_EMOJI, GUIDED_EMOJI, HELP_EMOJI, INFO_EMOJI,
    MENU_EMOJI, QUICK_EMOJI, SETTINGS_EMOJI, SYNC_EMOJI,
};

pub fn welcome_text(actor: &User) -> String {
    let role_hint = if actor.role.is_manager_or_admin() {
        "У вас открыт расширенный раздел команды: можно быстро смотреть review, блокеры и задачи, где нужно решение менеджера."
    } else {
        "Здесь удобно ставить задачи, держать фокус и быстро двигать работу дальше."
    };

    let display = actor.display_name_object();
    format!(
        "✨ Добро пожаловать, {}!\n\nЯ помогу поставить задачу, не потерять дедлайн и быстро понять, что требует внимания.\n{role_hint}\n\nВыберите, с чего начать:",
        display
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

/// Shown on the `GuidedAssigneeOptions` screen when the entered assignee text
/// is ambiguous or only partially matched.  Wraps the resolver's own message
/// (which describes whether it's a "did you mean" suggestion or a multi-choice
/// ambiguity) with the step-counter context so the user knows where they are.
pub fn guided_assignee_clarification_text(resolver_message: &str) -> String {
    format!("{GUIDED_EMOJI} Шаг 1 из 3\n\n{resolver_message}")
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

pub fn voice_interpretation_text(transcript: &str, preview: &TaskInterpretationPreview) -> String {
    let deadline = preview.deadline_label.as_deref().unwrap_or("без срока");
    let assignee = match &preview.assignee {
        AssigneeInterpretation::None => "Пока без исполнителя".to_owned(),
        AssigneeInterpretation::Resolved {
            display,
            delivery_status,
        } => format!(
            "{display}\nДоставка: {} — {}",
            delivery_badge(*delivery_status),
            delivery_detail(*delivery_status)
        ),
        AssigneeInterpretation::ClarificationRequired(request) => {
            let candidates = if request.candidates.is_empty() {
                "Нужно уточнить исполнителя вручную.".to_owned()
            } else {
                render_candidate_lines(&request.candidates)
            };
            format!("{}\n{}", request.message, candidates)
        }
    };

    // We deliberately show BOTH the raw transcript and the AI
    // reinterpretation so the user can always see exactly what the bot
    // heard and make an informed Edit / New-Recording decision.
    let transcript_block = if transcript.is_empty() {
        String::new()
    } else {
        format!("{INFO_EMOJI} Я услышал\n{transcript}\n\n")
    };

    format!(
        "{transcript_block}{INFO_EMOJI} Я понял задачу вот так\n\nИсполнитель:\n{assignee}\n\nСрок: {deadline}\n\nЧто нужно сделать:\n{}",
        preview.description
    )
}

pub fn voice_edit_prompt(transcript: &str) -> String {
    format!(
        "{GUIDED_EMOJI} Исправьте текст задачи\n\nСейчас у меня такая версия:\n{transcript}\n\nПришлите одним сообщением финальный текст задачи. Я заменю расшифровку и снова покажу подтверждение."
    )
}

pub fn registration_link_text(message: &str, candidates: &[EmployeeCandidateView]) -> String {
    if candidates.is_empty() {
        return format!(
            "{INFO_EMOJI} Привязка сотрудника\n\n{message}\n\nМожно продолжить без привязки и вернуться к этому позже."
        );
    }

    format!(
        "{INFO_EMOJI} Привязка сотрудника\n\n{message}\n\nПодходящие варианты:\n{}",
        render_candidate_lines(candidates)
    )
}

pub fn onboarding_welcome_text() -> String {
    format!(
        "{INFO_EMOJI} Добро пожаловать!\n\nЧтобы я мог корректно назначать вам задачи и подсказывать коллегам ваше имя, давайте быстро познакомимся.\n\nШаг 1 из 2. Пожалуйста, укажите ваше имя (одно слово, только буквы)."
    )
}

pub fn onboarding_ask_last_name_text(first_name: &str) -> String {
    format!(
        "{INFO_EMOJI} Отлично, {first_name}!\n\nШаг 2 из 2. Теперь пришлите вашу фамилию (одним сообщением, только буквы и дефис)."
    )
}

pub fn onboarding_retry_first_name_text() -> String {
    format!(
        "{INFO_EMOJI} Пожалуйста, пришлите ваше имя одним словом.\nИспользуйте только буквы (например: «Анна» или «Михаил»)."
    )
}

pub fn onboarding_retry_last_name_text() -> String {
    format!(
        "{INFO_EMOJI} Пришлите фамилию одной строкой.\nДопустимы буквы и дефис (например: «Иванова» или «Петров-Водкин»)."
    )
}

pub fn onboarding_too_long_text() -> String {
    format!(
        "{INFO_EMOJI} Слишком длинный текст.\nИмя и фамилия не должны превышать 64 символов. Попробуйте ещё раз."
    )
}

pub fn onboarding_link_expected_text() -> String {
    format!(
        "{INFO_EMOJI} Ожидаю ваш выбор на клавиатуре ниже.\nНажмите на свою карточку в справочнике или «Продолжить без привязки»."
    )
}

pub fn onboarding_completed_text(actor: &User) -> String {
    let display = actor.display_name_object();
    format!(
        "✨ Готово, {display}!\n\nПрофиль создан. Теперь откройте главное меню, чтобы поставить первую задачу или посмотреть список входящих."
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
    settings_text_with_stats(actor, None)
}

/// Same as [`settings_text`], but optionally renders the user's personal
/// task-activity summary right under the profile block.  When stats are
/// unavailable (failed query, no data yet) we deliberately skip the
/// section entirely so the profile screen still renders cleanly.
pub fn settings_text_with_stats(actor: &User, stats: Option<&StatsView>) -> String {
    let notifications = if actor.last_chat_id.is_some() {
        "подключены"
    } else {
        "не подключены"
    };
    let role = match actor.role {
        crate::domain::user::UserRole::User => "сотрудник",
        crate::domain::user::UserRole::Manager => "менеджер",
        crate::domain::user::UserRole::Admin => "администратор",
    };

    let display = actor.display_name_object();
    let fio = if matches!(
        display.kind(),
        crate::domain::user::DisplayNameKind::Anonymous
    ) {
        "не указано".to_owned()
    } else {
        display.as_str().to_owned()
    };

    let quiet_hours = format_quiet_hours(actor.quiet_hours_start_min, actor.quiet_hours_end_min);

    let profile = format!(
        "{SETTINGS_EMOJI} Профиль\n\n\
         Имя: {fio}\n\
         Username: {username}\n\
         Telegram ID: {telegram_id}\n\
         Роль: {role}\n\
         Часовой пояс: {tz}\n\
         Тихие часы: {quiet_hours}\n\
         Уведомления: {notifications}",
        username = actor.telegram_username.as_deref().unwrap_or("не указано"),
        telegram_id = actor.telegram_id,
        tz = actor.timezone,
    );

    match stats {
        Some(stats) => format!("{profile}\n\n{}", render_personal_activity(stats)),
        None => profile,
    }
}

fn render_personal_activity(stats: &StatsView) -> String {
    let average = stats
        .average_completion_hours
        .map(|value| format!("{value} ч"))
        .unwrap_or_else(|| "нет данных".to_owned());

    format!(
        "📊 Моя активность\n\n\
         Всего создано: {}\n\
         В работе: {}\n\
         Завершено: {}\n\
         Просрочено: {}\n\
         Среднее время выполнения: {}",
        stats.created_count,
        stats.active_count,
        stats.completed_count,
        stats.overdue_count,
        average,
    )
}

fn format_quiet_hours(start_min: i32, end_min: i32) -> String {
    if start_min == end_min {
        return "отключены".to_owned();
    }
    format!(
        "{start_hh:02}:{start_mm:02} – {end_hh:02}:{end_mm:02} (локальное время)",
        start_hh = (start_min / 60).rem_euclid(24),
        start_mm = (start_min % 60).rem_euclid(60),
        end_hh = (end_min / 60).rem_euclid(24),
        end_mm = (end_min % 60).rem_euclid(60),
    )
}

pub fn synced_text(count: usize) -> String {
    format!("{SYNC_EMOJI} Синхронизация завершена. Обновлено сотрудников: {count}.")
}

fn render_candidate_lines(candidates: &[EmployeeCandidateView]) -> String {
    candidates
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
}

#[cfg(test)]
mod tests {
    use super::{format_quiet_hours, settings_text, settings_text_with_stats};
    use crate::application::dto::task_views::StatsView;
    use crate::domain::user::{OnboardingState, User, UserRole};
    use chrono::Utc;

    fn make_user(first: Option<&str>, last: Option<&str>, role: UserRole) -> User {
        User {
            id: Some(1),
            telegram_id: 111,
            last_chat_id: Some(111),
            telegram_username: Some("ivanov".into()),
            full_name: Some("Ivan Ivanov".into()),
            first_name: first.map(str::to_owned),
            last_name: last.map(str::to_owned),
            linked_employee_id: None,
            is_employee: true,
            role,
            onboarding_state: OnboardingState::Completed,
            onboarding_version: 1,
            timezone: "Europe/Moscow".into(),
            quiet_hours_start_min: 22 * 60,
            quiet_hours_end_min: 8 * 60,
            deactivated_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn given_fio_when_rendered_then_prefers_first_plus_last_over_full_name() {
        let user = make_user(Some("Иван"), Some("Иванов"), UserRole::User);

        let text = settings_text(&user);

        assert!(text.contains("Имя: Иван Иванов"), "got: {text}");
        assert!(text.contains("Роль: сотрудник"), "got: {text}");
    }

    #[test]
    fn given_quiet_hours_when_enabled_then_formatted_with_zero_padding() {
        assert_eq!(
            format_quiet_hours(22 * 60, 8 * 60),
            "22:00 – 08:00 (локальное время)"
        );
    }

    #[test]
    fn given_quiet_hours_when_disabled_then_human_readable_off() {
        assert_eq!(format_quiet_hours(0, 0), "отключены");
    }

    #[test]
    fn given_profile_with_stats_when_rendered_then_activity_block_present() {
        let user = make_user(Some("Иван"), Some("Иванов"), UserRole::User);
        let stats = StatsView {
            created_count: 10,
            completed_count: 6,
            active_count: 3,
            overdue_count: 1,
            average_completion_hours: Some(12),
        };

        let text = settings_text_with_stats(&user, Some(&stats));

        assert!(text.contains("📊 Моя активность"), "got: {text}");
        assert!(text.contains("Всего создано: 10"), "got: {text}");
        assert!(text.contains("В работе: 3"), "got: {text}");
        assert!(text.contains("Завершено: 6"), "got: {text}");
        assert!(text.contains("Просрочено: 1"), "got: {text}");
        assert!(
            text.contains("Среднее время выполнения: 12 ч"),
            "got: {text}"
        );
    }

    #[test]
    fn given_profile_without_stats_when_rendered_then_no_activity_block() {
        let user = make_user(Some("Иван"), Some("Иванов"), UserRole::User);

        let text = settings_text_with_stats(&user, None);

        assert!(!text.contains("Моя активность"), "got: {text}");
    }

    #[test]
    fn given_stats_without_average_when_rendered_then_no_data_label() {
        let user = make_user(Some("A"), Some("B"), UserRole::User);
        let stats = StatsView {
            created_count: 0,
            completed_count: 0,
            active_count: 0,
            overdue_count: 0,
            average_completion_hours: None,
        };

        let text = settings_text_with_stats(&user, Some(&stats));

        assert!(
            text.contains("Среднее время выполнения: нет данных"),
            "got: {text}"
        );
    }

    #[test]
    fn given_admin_role_when_rendered_then_russian_label() {
        let mut user = make_user(Some("A"), Some("B"), UserRole::Admin);
        user.quiet_hours_start_min = 0;
        user.quiet_hours_end_min = 0;
        let text = settings_text(&user);

        assert!(text.contains("Роль: администратор"), "got: {text}");
        assert!(text.contains("Тихие часы: отключены"), "got: {text}");
    }
}
