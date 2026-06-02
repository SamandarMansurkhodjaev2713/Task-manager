//! Тексты для role-aware справки (`/help`).
//!
//! Архитектура:
//! - [`help_overview_text`] — корневой экран `/help`: приветствие + ролевая
//!   подсказка о доступных подразделах.  Список разделов рендерится
//!   клавиатурой ([`super::ui_keyboards::help_overview_keyboard`]); сам
//!   текст разделы не перечисляет, чтобы не дублировать кнопки.
//! - [`help_section_text`] — отдельные подразделы.  Все строки статические,
//!   функция чистая — это упрощает snapshot-тесты.
//!
//! Стиль:
//! - Обращение «вы» — строчное.
//! - Никакого Markdown — теги Telegram ломаются при пользовательском
//!   контенте; справка отправляется plain text без `parse_mode`.
//! - Каждый блок — заголовок с эмодзи + 3-7 коротких строк.
//! - Длина каждой секции < 3500 байт, чтобы оставался запас на
//!   автодополнения и trailing-пометки.

use crate::domain::user::{User, UserRole};
use crate::presentation::telegram::callbacks::HelpSection;

use super::super::ui_shared::HELP_EMOJI;

pub fn help_overview_text(actor: &User) -> String {
    let display = actor.display_name_object();
    let role_hint = match actor.role {
        UserRole::User => {
            "Здесь собраны короткие гайды по основным сценариям работы. \
             Выберите раздел ниже — внутри пошаговые инструкции и ответы на частые вопросы."
        }
        UserRole::Manager => {
            "Здесь собраны гайды по работе с задачами и отдельный раздел для менеджера: \
             командные списки, статистика и override статусов."
        }
        UserRole::Admin => {
            "Доступны все разделы справки, включая руководство администратора \
             с инструкциями по ролям, флагам, журналам, синхронизации и бэкапам."
        }
    };

    format!(
        "{HELP_EMOJI} Справка\n\n\
         Здравствуйте, {display}.\n\n\
         {role_hint}\n\n\
         Если в нужный момент понадобится подсказка — нажмите «❓ Помощь» из главного меню \
         или отправьте команду /help."
    )
}

/// Возвращает текст конкретного подраздела.  Чистая детерминированная функция —
/// snapshot-тестам и переводам это упрощает жизнь.
pub fn help_section_text(section: HelpSection) -> &'static str {
    match section {
        HelpSection::Tasks => HELP_TEXT_TASKS,
        HelpSection::Voice => HELP_TEXT_VOICE,
        HelpSection::Notifications => HELP_TEXT_NOTIFICATIONS,
        HelpSection::Manager => HELP_TEXT_MANAGER,
        HelpSection::Admin => HELP_TEXT_ADMIN,
    }
}

/// Заголовок подраздела — используется для алертов/тостов и snapshot-тестов.
pub fn help_section_title(section: HelpSection) -> &'static str {
    match section {
        HelpSection::Tasks => "📋 Задачи",
        HelpSection::Voice => "🎤 Голосовое создание",
        HelpSection::Notifications => "🔔 Уведомления",
        HelpSection::Manager => "🧭 Для менеджера",
        HelpSection::Admin => "🛡 Для администратора",
    }
}

/// Текст-«заглушка» для попытки открыть подраздел без необходимой роли.
/// Защита defence-in-depth: keyboard уже скрывает кнопку, но callback может
/// прийти от старого сообщения после демоута роли.
pub fn help_section_forbidden_text(section: HelpSection) -> String {
    let role_required = match section {
        HelpSection::Manager => "менеджеру или администратору",
        HelpSection::Admin => "администратору",
        // Универсальные разделы доступны всегда — этой ветки не должно
        // быть в реальных сценариях, но возвращаем понятное сообщение
        // вместо паники.
        HelpSection::Tasks | HelpSection::Voice | HelpSection::Notifications => {
            "пользователям с дополнительными правами"
        }
    };

    format!(
        "{} {}\n\n\
         Этот раздел доступен только {role_required}. \
         Если вам нужны эти возможности — обратитесь к администратору команды.",
        HELP_EMOJI,
        help_section_title(section),
    )
}

// ─── Содержимое подразделов ──────────────────────────────────────────────────

const HELP_TEXT_TASKS: &str = "📋 Задачи\n\n\
Как создать задачу — три способа:\n\
• Быстрый ввод: «🆕 Создать задачу» → «⚡ Быстрый режим» и одно сообщение текстом или голосом.\n\
• Пошагово: «🆕 Создать задачу» → «🧭 Пошагово». Бот по очереди спросит исполнителя, описание и срок.\n\
• Командой: /new_task @ivanov подготовить релиз до пятницы.\n\n\
Назначение исполнителя:\n\
• По @username — самый надёжный способ.\n\
• По имени или фамилии — бот ищет по справочнику; при совпадениях покажет варианты.\n\
• Без исполнителя — задача останется у вас, можно назначить позже.\n\n\
Сроки: «завтра», «пятница», «12.05», «через 3 дня», «без срока».\n\n\
Статусы задач:\n\
🆕 новая → 📨 отправлена → ▶️ в работе → 🧪 на проверке → ✅ завершена.\n\
🚧 есть блокер — задача стоит, ждёт решения.\n\
⛔ отменена — задача закрыта без выполнения.\n\n\
Что можно делать с задачей:\n\
• Менять статус кнопками в карточке задачи.\n\
• Сообщить о блокере — автор и менеджер получат уведомление.\n\
• Оставить комментарий — он попадёт в историю и уйдёт участникам.\n\
• Переназначить — на другого сотрудника (если вы автор задачи или менеджер).\n\
• Отменить — с подтверждением.\n\n\
Полезные команды:\n\
/my_tasks — мои задачи, /created_tasks — мной поставленные, /status T-0001 — открыть задачу по коду.";

const HELP_TEXT_VOICE: &str = "🎤 Голосовое создание задачи\n\n\
Как использовать:\n\
• Откройте «🆕 Создать задачу» → «⚡ Быстрый режим».\n\
• Запишите голосовое прямо в чат бота (зажмите микрофон).\n\
• Бот покажет распознанный текст и предположение — кого, что и к какому сроку.\n\
• Вы подтверждаете, правите текст или отменяете.\n\n\
Что бот старается распознать:\n\
• Исполнителя — по имени или @username, упомянутому в записи.\n\
• Срок — фразы «завтра», «к пятнице», «через 3 дня», «12 мая».\n\
• Описание — основной смысл задачи.\n\n\
Как говорить, чтобы получалось точнее:\n\
• Сначала имя исполнителя, потом задача, потом срок.\n\
• Один абзац — одна задача. Несколько задач за раз бот не разделит.\n\
• Без длинных пауз — Telegram режет долгие записи.\n\n\
Если бот ошибся:\n\
• «✏️ Исправить текст» — пришлите финальный текст одним сообщением, бот заменит расшифровку.\n\
• «❌ Отменить» — закрыть без сохранения.\n\
• «✅ Создать задачу» — сохранить как есть.\n\n\
Когда лучше не использовать голос:\n\
• Очень шумное окружение — распознавание заметно падает.\n\
• Сложные технические термины и аббревиатуры — лучше написать текстом.\n\
• Длинные описания (более минуты) — бот может не успеть обработать.";

const HELP_TEXT_NOTIFICATIONS: &str = "🔔 Уведомления\n\n\
Что бот присылает по своей инициативе:\n\
• 📨 Назначение задачи — как только вас выбрали исполнителем.\n\
• 💬 Комментарий — когда участник задачи добавил новое сообщение.\n\
• 🚧 Блокер — автору и менеджеру, когда исполнитель отметил блокер.\n\
• 🧪 Отправка на проверку — автору задачи.\n\
• ✅ Завершение / ⛔ отмена — всем участникам.\n\
• 📅 Напоминание о сроке — за день до дедлайна (по умолчанию утром).\n\
• ⚠️ Просрочка — если срок прошёл, а задача в работе.\n\
• 📋 Ежедневный дайджест — список ваших активных задач (по умолчанию утром).\n\n\
Тихие часы:\n\
• Текущее значение — в «⚙️ Профиль» (строка «Тихие часы»).\n\
• Изменить тихие часы или часовой пояс может только администратор.\n\
• В тихие часы бот не присылает обычные уведомления, но критичные события \
(блокер, просрочка важной задачи) проходят.\n\
• Часовой пояс берётся из профиля. По умолчанию Europe/Moscow.\n\n\
Как остановить уведомления полностью:\n\
• Заблокировать бота в Telegram — все уведомления остановятся, но и задачи перестанут приходить.\n\
• Лучше — настроить тихие часы или попросить администратора деактивировать ваш аккаунт.\n\n\
Если уведомления не приходят:\n\
• Проверьте, не заблокирован ли бот в Telegram.\n\
• Проверьте, что вы прошли /start (без этого бот не знает, в какой чат писать).\n\
• Откройте «⚙️ Профиль» — поле «Уведомления» должно быть «подключены».";

const HELP_TEXT_MANAGER: &str = "🧭 Для менеджера\n\n\
Дополнительные возможности по сравнению с обычным сотрудником:\n\n\
👥 Командные задачи — полный список задач команды, не только ваших.\n\
🧪 Inbox менеджера — задачи, в которых нужно ваше решение: на проверке, \
с блокером, без исполнителя.\n\
📈 Командная статистика — сводка по всей команде: создано, активных, \
завершено, просрочено.\n\n\
Override статусов:\n\
• Менеджер может принудительно перевести любую задачу в «в работе», «завершена» или «отменена».\n\
• Используйте это, чтобы разблокировать ситуацию, когда исполнитель недоступен.\n\
• Действие фиксируется в истории задачи — кто и когда менял статус.\n\n\
Переназначение чужих задач:\n\
• Менеджер может переназначить любую задачу команды на другого сотрудника.\n\
• Старый исполнитель получит уведомление о снятии задачи.\n\
• Новый исполнитель получит уведомление о назначении.\n\n\
Когда вмешиваться в чужие задачи:\n\
• Исполнитель сообщил о блокере, который требует вашего решения.\n\
• Сроки горят, а исполнитель не отвечает.\n\
• Задача висит в «на проверке» больше суток — нужно либо принять, либо вернуть в работу.\n\n\
Когда лучше НЕ вмешиваться:\n\
• Если автор задачи активен и сам ведёт её — позвольте ему закрыть.\n\
• Если просто хотите «посмотреть» — лучше открыть карточку без изменения статуса.\n\n\
Командная статистика:\n\
• Цифры обновляются с задержкой до 60 секунд (кеш).\n\
• Учитываются все задачи команды независимо от автора.";

const HELP_TEXT_ADMIN: &str = "🛡 Для администратора\n\n\
Панель: /admin или кнопка из главного меню.\n\n\
Управление пользователями («👥 Администраторы»):\n\
• Сменить роль: пользователь / менеджер / администратор.\n\
• Деактивировать — аккаунт не может создавать и менять задачи; задачи сохраняются.\n\
• Реактивировать — снимает блокировку.\n\
• Защита last-admin: нельзя деактивировать или демоутнуть единственного активного админа.\n\
• Действия к самому себе запрещены — попросите другого администратора.\n\
• Каждая операция требует подтверждения; кнопка живёт 2 минуты.\n\n\
Журналы:\n\
• 📜 Журнал действий — последние изменения ролей, деактиваций, переключений флагов.\n\
• 🔐 Журнал безопасности — отказы в доступе, чужие callback'и, превышения rate-limit.\n\n\
Флаги функций («🚩 Флаги функций», изменения применяются сразу):\n\
• onboarding_v2 — пошаговая регистрация. Отключение сломает /start.\n\
• admin_panel — сама панель администратора. Отключение закроет её до перезапуска.\n\
• sla_escalations — алерты о нарушении SLA.\n\
• voice_v2 — расширенное голосовое (заглушка, по умолчанию выключено).\n\
• task_templates — шаблоны задач.\n\
• recurrence_rules — повторяющиеся задачи.\n\
• team_analytics — командная статистика для менеджеров.\n\n\
Синхронизация сотрудников:\n\
• «🔄 Синхронизировать сотрудников» или /admin_sync_employees.\n\
• Подтягивает свежий список из Google Sheets / CSV.\n\
• Существующие @username обновляются, не дублируются.\n\n\
Полный сброс справочника:\n\
• В .env установить RESET_EMPLOYEES_ON_STARTUP=true и перезапустить.\n\
• ВАЖНО: вернуть в false после старта, иначе сотрудники сбрасываются на каждом перезапуске.\n\
• При сбросе FK-ссылки в задачах и пользователях обнуляются — задачи остаются, исполнитель снимается.\n\n\
Бэкап и восстановление:\n\
• Файл БД: data/app.db (SQLite).\n\
• Регулярно снимайте копию (горячий backup через sqlite .backup или утилиту).\n\
• Восстановление: остановить бота, заменить data/app.db на копию, запустить.\n\n\
Диагностика, если бот не отвечает:\n\
• /healthz — проверка процесса.\n\
• /healthz/deep — проверка БД с latency.\n\
• /metrics — Prometheus-метрики.\n\
• /version — версия и git_sha работающего инстанса.";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::user::{OnboardingState, User, UserRole};
    use chrono::Utc;

    fn make_user(role: UserRole) -> User {
        User {
            id: Some(1),
            telegram_id: 111,
            last_chat_id: Some(111),
            telegram_username: Some("ivanov".into()),
            full_name: Some("Иван Иванов".into()),
            first_name: Some("Иван".into()),
            last_name: Some("Иванов".into()),
            linked_employee_id: None,
            is_employee: true,
            role,
            onboarding_state: OnboardingState::Completed,
            onboarding_version: 1,
            timezone: "Europe/Moscow".into(),
            quiet_hours_start_min: 0,
            quiet_hours_end_min: 0,
            deactivated_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn given_user_role_when_overview_then_user_only_hint_present() {
        let user = make_user(UserRole::User);
        let text = help_overview_text(&user);

        assert!(
            text.contains("гайды по основным сценариям"),
            "user-role overview must use the user-targeted hint; got: {text}"
        );
        assert!(!text.contains("раздел для менеджера"));
        assert!(!text.contains("руководство администратора"));
    }

    #[test]
    fn given_manager_role_when_overview_then_manager_hint_present() {
        let user = make_user(UserRole::Manager);
        let text = help_overview_text(&user);

        assert!(
            text.contains("раздел для менеджера"),
            "manager-role overview must mention manager hint; got: {text}"
        );
        assert!(!text.contains("руководство администратора"));
    }

    #[test]
    fn given_admin_role_when_overview_then_admin_hint_present() {
        let user = make_user(UserRole::Admin);
        let text = help_overview_text(&user);

        assert!(
            text.contains("руководство администратора"),
            "admin-role overview must mention admin hint; got: {text}"
        );
    }

    #[test]
    fn given_each_section_when_text_then_starts_with_correct_title() {
        // Защита от случайной перепутанной маршрутизации текстов.
        for (section, title_marker) in [
            (HelpSection::Tasks, "📋 Задачи"),
            (HelpSection::Voice, "🎤 Голосовое создание"),
            (HelpSection::Notifications, "🔔 Уведомления"),
            (HelpSection::Manager, "🧭 Для менеджера"),
            (HelpSection::Admin, "🛡 Для администратора"),
        ] {
            let text = help_section_text(section);
            assert!(
                text.starts_with(title_marker),
                "section {section:?} text must start with title '{title_marker}'; got: {text}"
            );
        }
    }

    #[test]
    fn given_each_section_when_text_then_under_telegram_message_limit() {
        // Telegram обрезает сообщения длиннее 4096 байт; держим запас на хвостовые
        // строки, которые мы можем добавлять в рендерере (например, navigation hints).
        const SAFE_LIMIT: usize = 3800;
        for section in [
            HelpSection::Tasks,
            HelpSection::Voice,
            HelpSection::Notifications,
            HelpSection::Manager,
            HelpSection::Admin,
        ] {
            let text = help_section_text(section);
            assert!(
                text.len() <= SAFE_LIMIT,
                "section {section:?} text length {} exceeds safe limit {SAFE_LIMIT}",
                text.len()
            );
        }
    }

    #[test]
    fn given_admin_section_when_forbidden_for_user_then_visibility_check_returns_false() {
        assert!(!HelpSection::Admin.is_visible_to(UserRole::User));
        assert!(!HelpSection::Admin.is_visible_to(UserRole::Manager));
        assert!(HelpSection::Admin.is_visible_to(UserRole::Admin));
    }

    #[test]
    fn given_manager_section_when_visibility_then_manager_and_admin_only() {
        assert!(!HelpSection::Manager.is_visible_to(UserRole::User));
        assert!(HelpSection::Manager.is_visible_to(UserRole::Manager));
        assert!(HelpSection::Manager.is_visible_to(UserRole::Admin));
    }

    #[test]
    fn given_universal_sections_when_visibility_then_all_roles_pass() {
        for section in [
            HelpSection::Tasks,
            HelpSection::Voice,
            HelpSection::Notifications,
        ] {
            for role in [UserRole::User, UserRole::Manager, UserRole::Admin] {
                assert!(
                    section.is_visible_to(role),
                    "{section:?} must be visible to {role:?}"
                );
            }
        }
    }

    #[test]
    fn given_admin_forbidden_text_when_user_targets_admin_section_then_mentions_admin_role() {
        let text = help_section_forbidden_text(HelpSection::Admin);
        assert!(
            text.contains("администратору"),
            "admin-section refusal must name the admin role; got: {text}"
        );
    }

    #[test]
    fn given_manager_forbidden_text_when_user_targets_manager_section_then_mentions_both_roles() {
        let text = help_section_forbidden_text(HelpSection::Manager);
        assert!(
            text.contains("менеджеру") && text.contains("администратору"),
            "manager-section refusal must name both eligible roles; got: {text}"
        );
    }
}
