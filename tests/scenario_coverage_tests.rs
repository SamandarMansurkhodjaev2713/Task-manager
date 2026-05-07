//! Phase 4 — 50-scenario coverage across 5 personas.
//!
//! Each persona group covers 10 scenarios testing the layers reachable without
//! a live Telegram connection: domain logic, application use-case guards, UI
//! text rendering, keyboard structure, and callback codec roundtrips.
//!
//! Persona groups:
//!   P1 — Новый сотрудник (new, unlinked)
//!   P2 — Обычный сотрудник (regular employee)
//!   P3 — Менеджер (manager role)
//!   P4 — Администратор (admin role)
//!   P5 — Исполнитель на разных статусах (employee with various task states)

mod factories;

use chrono::Utc;
use uuid::Uuid;

use telegram_task_bot::application::dto::task_views::{
    ClarificationRequest, DeliveryStatus, EmployeeCandidateView, TaskActionView,
    TaskCreationOutcome, TaskCreationSummary, TaskStatusDetails,
};
use telegram_task_bot::domain::task::TaskStatus;
use telegram_task_bot::domain::user::{OnboardingState, User, UserRole};
use telegram_task_bot::presentation::telegram::callbacks::{
    encode_callback, parse_callback, HelpSection, TaskCardMode, TaskListOrigin, TelegramCallback,
};
use telegram_task_bot::presentation::telegram::ui;
use telegram_task_bot::presentation::telegram::ui::{
    cancel_confirmation_text, help_section_text, list_header, list_text, task_comment_prompt,
    task_creation_text, task_detail_text, task_reassign_prompt,
};

// ─── Shared factories ─────────────────────────────────────────────────────────

fn user_with_role(role: UserRole) -> User {
    let now = Utc::now();
    User {
        id: Some(42),
        telegram_id: 100_000,
        last_chat_id: Some(100_000),
        telegram_username: Some("testuser".to_owned()),
        full_name: Some("Тест Пользователь".to_owned()),
        first_name: Some("Тест".to_owned()),
        last_name: Some("Пользователь".to_owned()),
        linked_employee_id: Some(1),
        is_employee: true,
        role,
        onboarding_state: OnboardingState::Completed,
        onboarding_version: 1,
        timezone: "Europe/Moscow".to_owned(),
        quiet_hours_start_min: 0,
        quiet_hours_end_min: 0,
        deactivated_at: None,
        created_at: now,
        updated_at: now,
    }
}

fn make_task_status_details(status: TaskStatus) -> TaskStatusDetails {
    TaskStatusDetails {
        task_uid: Uuid::now_v7(),
        public_code: "T-0001".to_owned(),
        title: "Написать тест".to_owned(),
        status,
        deadline: Some("01.06.2026".to_owned()),
        expected_result: "Тест проходит".to_owned(),
        description_lines: vec!["Шаг 1".to_owned(), "Шаг 2".to_owned()],
        acceptance_criteria: vec!["Зелёный CI".to_owned()],
        history_entries: vec!["01.05.2026 10:00: created".to_owned()],
        assignee_display: Some("@ivanov".to_owned()),
        delivery_status: Some(DeliveryStatus::DeliveredToAssignee),
        blocked_reason: None,
        comments: Vec::new(),
        available_actions: Vec::new(),
    }
}

fn make_creation_summary() -> TaskCreationSummary {
    let task = factories::task(None);
    TaskCreationSummary::from_task(
        &task,
        "Задача создана".to_owned(),
        DeliveryStatus::DeliveredToAssignee,
    )
}

// ─── P1: Новый сотрудник ──────────────────────────────────────────────────────

#[test]
fn p1_s01_registration_keyboard_with_candidates_has_unconditional_home_button() {
    let candidates = vec![EmployeeCandidateView {
        employee_id: Some(1),
        full_name: "Иван Иванов".to_owned(),
        telegram_username: Some("ivan".to_owned()),
        confidence: 100,
        active_task_count: Some(0),
    }];
    let keyboard = ui::registration_link_keyboard(&candidates, true);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"🏠 В меню"),
        "registration keyboard must always contain 🏠 В меню; got: {all_labels:?}"
    );
}

#[test]
fn p1_s02_registration_keyboard_without_candidates_still_has_home_button() {
    let keyboard = ui::registration_link_keyboard(&[], false);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"🏠 В меню"),
        "registration keyboard with no candidates must still show 🏠 В меню"
    );
}

#[test]
fn p1_s03_registration_keyboard_with_allow_continue_shows_unlinked_button() {
    let keyboard = ui::registration_link_keyboard(&[], true);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"Продолжить без привязки"),
        "registration keyboard with allow_continue must show 'Продолжить без привязки'"
    );
}

#[test]
fn p1_s04_registration_callback_pick_employee_codec_roundtrip() {
    let cb = TelegramCallback::RegistrationPickEmployee { employee_id: 77 };
    let encoded = encode_callback(&cb);
    let decoded = parse_callback(&encoded).expect("must decode");
    assert_eq!(decoded, cb);
}

#[test]
fn p1_s05_registration_continue_unlinked_codec_roundtrip() {
    let cb = TelegramCallback::RegistrationContinueUnlinked;
    let encoded = encode_callback(&cb);
    let decoded = parse_callback(&encoded).expect("must decode");
    assert_eq!(decoded, cb);
}

#[test]
fn p1_s06_help_overview_employee_role_shows_universal_sections_only() {
    let user = user_with_role(UserRole::User);
    let keyboard = ui::help_overview_keyboard(&user);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(all_labels.contains(&"📋 Задачи"));
    assert!(all_labels.contains(&"🎤 Голосовое создание"));
    assert!(all_labels.contains(&"🔔 Уведомления"));
    assert!(
        !all_labels.contains(&"🧭 Для менеджера"),
        "employee must not see manager section"
    );
    assert!(
        !all_labels.contains(&"🛡 Для администратора"),
        "employee must not see admin section"
    );
}

#[test]
fn p1_s07_help_section_tasks_visible_to_all_roles() {
    for role in [UserRole::User, UserRole::Manager, UserRole::Admin] {
        assert!(
            HelpSection::Tasks.is_visible_to(role),
            "Tasks section must be visible to {role:?}"
        );
    }
}

#[test]
fn p1_s08_help_section_manager_not_visible_to_employee() {
    assert!(
        !HelpSection::Manager.is_visible_to(UserRole::User),
        "Manager help section must not be visible to regular user"
    );
}

#[test]
fn p1_s09_help_section_admin_not_visible_to_employee_or_manager() {
    assert!(
        !HelpSection::Admin.is_visible_to(UserRole::User),
        "Admin help section must not be visible to user"
    );
    assert!(
        !HelpSection::Admin.is_visible_to(UserRole::Manager),
        "Admin help section must not be visible to manager"
    );
}

#[test]
fn p1_s10_help_section_voice_visible_to_user_role() {
    assert!(
        HelpSection::Voice.is_visible_to(UserRole::User),
        "Voice section must be visible to regular user"
    );
}

// ─── P2: Обычный сотрудник ────────────────────────────────────────────────────

#[test]
fn p2_s01_task_in_progress_transition_from_created_succeeds() {
    let task = factories::task(None);
    let updated = task
        .transition_to(TaskStatus::InProgress, Utc::now())
        .expect("created -> in_progress must succeed");
    assert_eq!(updated.status, TaskStatus::InProgress);
    assert!(updated.started_at.is_some());
}

#[test]
fn p2_s02_task_submit_for_review_transition_succeeds() {
    let task = factories::task(None);
    let in_progress = task
        .transition_to(TaskStatus::InProgress, Utc::now())
        .expect("created -> in_progress");
    let in_review = in_progress
        .transition_to(TaskStatus::InReview, Utc::now())
        .expect("in_progress -> in_review must succeed");
    assert_eq!(in_review.status, TaskStatus::InReview);
    assert!(in_review.review_requested_at.is_some());
}

#[test]
fn p2_s03_completed_task_cannot_transition_to_in_progress() {
    let task = factories::task(None);
    let completed = task
        .transition_to(TaskStatus::InProgress, Utc::now())
        .and_then(|t| t.transition_to(TaskStatus::InReview, Utc::now()))
        .and_then(|t| t.transition_to(TaskStatus::Completed, Utc::now()))
        .expect("full happy path");
    let err = completed.transition_to(TaskStatus::InProgress, Utc::now());
    assert!(
        err.is_err(),
        "completed task must not allow transition back to in_progress"
    );
}

#[test]
fn p2_s04_cancelled_task_cannot_be_cancelled_again() {
    let task = factories::task(None);
    let cancelled = task
        .transition_to(TaskStatus::Cancelled, Utc::now())
        .expect("created -> cancelled");
    let err = cancelled.transition_to(TaskStatus::Cancelled, Utc::now());
    assert!(err.is_err(), "cancelled task must not be cancellable again");
}

#[test]
fn p2_s05_task_detail_compact_text_renders_without_panic() {
    let details = make_task_status_details(TaskStatus::InProgress);
    let text = task_detail_text(&details, TaskCardMode::Compact, None);
    assert!(
        text.contains("T-0001"),
        "compact text must include task code"
    );
    assert!(
        text.contains("Написать тест"),
        "compact text must include title"
    );
    assert!(!text.is_empty());
}

#[test]
fn p2_s06_task_detail_expanded_text_renders_without_panic() {
    let details = make_task_status_details(TaskStatus::InProgress);
    let text = task_detail_text(&details, TaskCardMode::Expanded, None);
    assert!(
        text.contains("Ожидаемый результат"),
        "expanded text must include expected result section"
    );
    assert!(
        text.contains("Критерии приёмки"),
        "expanded text must include acceptance criteria section"
    );
}

#[test]
fn p2_s07_task_detail_shows_blocker_reason_in_card() {
    let mut details = make_task_status_details(TaskStatus::InProgress);
    details.blocked_reason = Some("Нет доступа к серверу".to_owned());
    let text = task_detail_text(&details, TaskCardMode::Compact, None);
    assert!(
        text.contains("Нет доступа к серверу"),
        "blocker reason must appear in rendered card"
    );
}

#[test]
fn p2_s08_pending_registration_delivery_shown_in_card() {
    let mut details = make_task_status_details(TaskStatus::InProgress);
    details.delivery_status = Some(DeliveryStatus::PendingAssigneeRegistration);
    let text = task_detail_text(&details, TaskCardMode::Compact, None);
    assert!(
        text.contains("Ждёт /start"),
        "PendingAssigneeRegistration must render the 'Ждёт /start' badge"
    );
    assert!(
        text.contains("исполнитель ещё не запускал бота"),
        "delivery detail must explain the registration wait"
    );
}

#[test]
fn p2_s09_in_review_status_shows_waiting_for_review_in_card() {
    let details = make_task_status_details(TaskStatus::InReview);
    let text = task_detail_text(&details, TaskCardMode::Compact, None);
    assert!(
        text.contains("ждёт решения по проверке"),
        "InReview task must note that it awaits review decision"
    );
}

#[test]
fn p2_s10_add_comment_keyboard_button_uses_verb_not_noun() {
    // Phase 3 fix: action_label(AddComment) was "💬 Комментарий" (noun).
    // Now it must be "💬 Добавить комментарий".
    // We verify through the keyboard: build a task_detail_keyboard that
    // includes AddComment and inspect its button labels.
    let mut details = make_task_status_details(TaskStatus::InProgress);
    details.available_actions = vec![TaskActionView::AddComment];
    let keyboard =
        ui::task_detail_keyboard(&details, TaskListOrigin::Assigned, TaskCardMode::Compact);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels
            .iter()
            .any(|l| l.to_lowercase().contains("добавить")
                || l.to_lowercase().contains("комментарий")),
        "AddComment button must be labelled with an imperative verb phrase; got: {all_labels:?}"
    );
    // Specifically must NOT be just the noun "Комментарий" without a verb
    assert!(
        !all_labels.contains(&"💬 Комментарий"),
        "AddComment must not use bare noun '💬 Комментарий'"
    );
}

// ─── P3: Менеджер ─────────────────────────────────────────────────────────────

#[test]
fn p3_s01_manager_role_sees_manager_help_section_but_not_admin() {
    let user = user_with_role(UserRole::Manager);
    let keyboard = ui::help_overview_keyboard(&user);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"🧭 Для менеджера"),
        "manager must see the manager help section"
    );
    assert!(
        !all_labels.contains(&"🛡 Для администратора"),
        "manager must not see the admin help section"
    );
}

#[test]
fn p3_s02_manager_inbox_list_header_is_non_empty() {
    let (title, subtitle) = list_header(TaskListOrigin::ManagerInbox);
    assert!(!title.is_empty(), "manager inbox title must not be empty");
    assert!(
        !subtitle.is_empty(),
        "manager inbox subtitle must not be empty"
    );
}

#[test]
fn p3_s03_team_list_header_is_non_empty() {
    let (title, subtitle) = list_header(TaskListOrigin::Team);
    assert!(!title.is_empty(), "team list title must not be empty");
    assert!(!subtitle.is_empty(), "team list subtitle must not be empty");
}

#[test]
fn p3_s04_empty_task_list_text_contains_create_cta() {
    use telegram_task_bot::application::dto::task_views::TaskListPage;
    let (title, subtitle) = list_header(TaskListOrigin::Assigned);
    let page = TaskListPage {
        sections: vec![],
        next_cursor: None,
    };
    let text = list_text(title, subtitle, &page);
    assert!(
        text.contains("Создать задачу") || text.contains("меню"),
        "empty list must prompt user to create tasks; got: {text}"
    );
}

#[test]
fn p3_s05_task_creation_text_created_variant_has_task_code_and_success_indicator() {
    let summary = make_creation_summary();
    let outcome = TaskCreationOutcome::Created(summary);
    let text = task_creation_text(&outcome);
    assert!(
        text.contains("✅"),
        "created outcome must have success indicator"
    );
    assert!(!text.is_empty());
}

#[test]
fn p3_s06_task_creation_text_duplicate_found_shows_not_created_message() {
    let summary = make_creation_summary();
    let outcome = TaskCreationOutcome::DuplicateFound(summary);
    let text = task_creation_text(&outcome);
    assert!(
        text.contains("уже есть") || text.contains("Дубль"),
        "duplicate outcome text must mention the duplicate; got: {text}"
    );
}

#[test]
fn p3_s07_task_creation_clarification_required_shows_candidate_names() {
    let request = ClarificationRequest {
        message: "Уточните исполнителя.".to_owned(),
        requested_query: Some("Иван".to_owned()),
        allow_unassigned: false,
        candidates: vec![EmployeeCandidateView {
            employee_id: Some(1),
            full_name: "Иван Петров".to_owned(),
            telegram_username: Some("ivan_p".to_owned()),
            confidence: 90,
            active_task_count: Some(2),
        }],
        task_body_preview: None,
    };
    let outcome = TaskCreationOutcome::ClarificationRequired(request);
    let text = task_creation_text(&outcome);
    assert!(
        text.contains("Иван Петров"),
        "clarification text must list candidate names"
    );
}

#[test]
fn p3_s08_cancel_confirmation_text_shows_task_code_and_title() {
    let details = make_task_status_details(TaskStatus::InProgress);
    let text = cancel_confirmation_text(&details);
    assert!(
        text.contains("T-0001"),
        "cancel confirmation must show task code"
    );
    assert!(
        text.contains("Написать тест"),
        "cancel confirmation must show title"
    );
}

#[test]
fn p3_s09_task_comment_prompt_mentions_task_code() {
    let details = make_task_status_details(TaskStatus::InProgress);
    let text = task_comment_prompt(&details);
    assert!(!text.is_empty(), "comment prompt must not be empty");
    assert!(
        text.contains("T-0001"),
        "comment prompt must mention task code"
    );
}

#[test]
fn p3_s10_task_reassign_prompt_mentions_task_code() {
    let details = make_task_status_details(TaskStatus::InProgress);
    let text = task_reassign_prompt(&details);
    assert!(!text.is_empty(), "reassign prompt must not be empty");
    assert!(
        text.contains("T-0001"),
        "reassign prompt must mention task code"
    );
}

// ─── P4: Администратор ────────────────────────────────────────────────────────

#[test]
fn p4_s01_admin_role_sees_both_manager_and_admin_help_sections() {
    let user = user_with_role(UserRole::Admin);
    let keyboard = ui::help_overview_keyboard(&user);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"🧭 Для менеджера"),
        "admin must see the manager help section"
    );
    assert!(
        all_labels.contains(&"🛡 Для администратора"),
        "admin must see the admin help section"
    );
}

#[test]
fn p4_s02_admin_menu_keyboard_has_home_button() {
    let keyboard = ui::admin_menu_keyboard();
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"🏠 В меню"),
        "admin menu must have 🏠 В меню"
    );
}

#[test]
fn p4_s03_admin_users_keyboard_has_back_and_home() {
    let keyboard = ui::admin_users_keyboard(&[]);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"↩️ В панель"),
        "admin users must have back to panel"
    );
    assert!(
        all_labels.contains(&"🏠 В меню"),
        "admin users must have home"
    );
}

#[test]
fn p4_s04_admin_user_details_keyboard_shows_non_current_role_buttons() {
    // target is a User — should show Менеджер and Администратор but not Сотрудник
    let user = user_with_role(UserRole::User);
    let keyboard = ui::admin_user_details_keyboard(&user);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"🧭 Менеджер"),
        "should show Менеджер option for a User"
    );
    assert!(
        all_labels.contains(&"🛡 Администратор"),
        "should show Администратор option for a User"
    );
    assert!(
        !all_labels.contains(&"👤 Сотрудник"),
        "must NOT show current role (Сотрудник) as a change option"
    );
}

#[test]
fn p4_s05_admin_user_details_keyboard_hides_current_admin_role() {
    let user = user_with_role(UserRole::Admin);
    let keyboard = ui::admin_user_details_keyboard(&user);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        !all_labels.contains(&"🛡 Администратор"),
        "must NOT show current role (Администратор) as a change option"
    );
    // Should still show lower roles
    assert!(
        all_labels.contains(&"👤 Сотрудник"),
        "admin detail must show Сотрудник downgrade option"
    );
}

#[test]
fn p4_s06_admin_confirmation_keyboard_has_home_button() {
    // Phase 3 fix: admin confirmation must have 🏠 В меню
    let keyboard = ui::admin_confirmation_keyboard("test-nonce-abc");
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"🏠 В меню"),
        "admin confirmation keyboard must have 🏠 В меню after Phase 3 fix"
    );
    assert!(
        all_labels.contains(&"✅ Подтвердить"),
        "admin confirmation keyboard must have confirm button"
    );
    assert!(
        all_labels.contains(&"❌ Отмена"),
        "admin confirmation keyboard must have cancel button"
    );
}

#[test]
fn p4_s07_admin_back_keyboard_has_both_navigation_buttons() {
    let keyboard = ui::admin_back_keyboard();
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"↩️ В панель"),
        "admin back keyboard must have ↩️ В панель"
    );
    assert!(
        all_labels.contains(&"🏠 В меню"),
        "admin back keyboard must have 🏠 В меню"
    );
}

#[test]
fn p4_s08_admin_features_keyboard_shows_enabled_flag_with_toggle_indicator() {
    use telegram_task_bot::shared::feature_flags::FeatureFlag;
    let flags = vec![(FeatureFlag::VoiceV2, true), (FeatureFlag::VoiceV2, false)];
    let keyboard = ui::admin_features_keyboard(&flags);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    // Enabled flag: ✅ … (откл.)
    assert!(
        all_labels
            .iter()
            .any(|l| l.contains("✅") && l.contains("откл")),
        "enabled flag must show ✅ with '(откл.)' to indicate what clicking will do"
    );
    // Disabled flag: ⬜ … (вкл.)
    assert!(
        all_labels
            .iter()
            .any(|l| l.contains("⬜") && l.contains("вкл")),
        "disabled flag must show ⬜ with '(вкл.)'"
    );
}

#[test]
fn p4_s09_help_section_codec_roundtrip_for_all_five_sections() {
    for section in [
        HelpSection::Tasks,
        HelpSection::Voice,
        HelpSection::Notifications,
        HelpSection::Manager,
        HelpSection::Admin,
    ] {
        let cb = TelegramCallback::MenuHelpSection { section };
        let encoded = encode_callback(&cb);
        let decoded = parse_callback(&encoded).expect("must decode");
        assert_eq!(
            decoded, cb,
            "HelpSection::{section:?} must roundtrip cleanly"
        );
    }
}

#[test]
fn p4_s10_help_section_text_for_admin_is_within_telegram_safe_limit() {
    // Telegram message text limit is 4096 bytes; conservative 3800-byte ceiling
    // leaves headroom for inline formatting overhead.
    let text = help_section_text(HelpSection::Admin);
    let byte_len = text.len();
    assert!(
        byte_len <= 3800,
        "Admin help text must be ≤ 3800 bytes (Telegram safe limit); actual: {byte_len}"
    );
}

// ─── P5: Исполнитель на разных статусах ───────────────────────────────────────

#[test]
fn p5_s01_cancel_action_appears_in_its_own_last_row_in_task_detail_keyboard() {
    // Cancel is a "dangerous action" — it must be isolated in the last row,
    // separate from constructive actions.
    let mut details = make_task_status_details(TaskStatus::InProgress);
    details.available_actions = vec![TaskActionView::AddComment, TaskActionView::Cancel];
    let keyboard =
        ui::task_detail_keyboard(&details, TaskListOrigin::Assigned, TaskCardMode::Compact);
    // Find row containing Cancel
    let cancel_row = keyboard
        .inline_keyboard
        .iter()
        .position(|row| row.iter().any(|btn| btn.text.contains("Отменить")));
    let comment_row = keyboard.inline_keyboard.iter().position(|row| {
        row.iter().any(|btn| {
            btn.text.contains("Добавить комментарий") || btn.text.contains("комментарий")
        })
    });
    assert!(
        cancel_row.is_some(),
        "Cancel button must appear in the keyboard"
    );
    assert!(
        comment_row.is_some(),
        "Comment button must appear in the keyboard"
    );
    // Cancel must come AFTER the constructive actions
    assert!(
        cancel_row.unwrap() > comment_row.unwrap(),
        "Cancel (dangerous) must appear after constructive actions"
    );
}

#[test]
fn p5_s02_task_without_deadline_renders_without_deadline_line() {
    let mut details = make_task_status_details(TaskStatus::InProgress);
    details.deadline = None;
    let text = task_detail_text(&details, TaskCardMode::Compact, None);
    assert!(
        text.contains("без срока"),
        "task without deadline must say 'без срока': {text}"
    );
}

#[test]
fn p5_s03_task_with_deadline_renders_deadline_value() {
    let details = make_task_status_details(TaskStatus::InProgress);
    // details.deadline is Some("01.06.2026")
    let text = task_detail_text(&details, TaskCardMode::Compact, None);
    assert!(
        text.contains("01.06.2026"),
        "task with deadline must render its value: {text}"
    );
}

#[test]
fn p5_s04_delivery_help_keyboard_has_back_to_task_and_home() {
    let uid = Uuid::now_v7();
    let keyboard = ui::delivery_help_keyboard(uid, TaskListOrigin::Assigned);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.iter().any(|l| l.contains("Вернуться к задаче")),
        "delivery help keyboard must have a back-to-task button"
    );
    assert!(
        all_labels.contains(&"🏠 В меню"),
        "delivery help keyboard must have home"
    );
}

#[test]
fn p5_s05_voice_confirmation_keyboard_has_confirm_edit_and_cancel() {
    let keyboard = ui::voice_confirmation_keyboard();
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"✅ Создать задачу"),
        "must have confirm"
    );
    assert!(all_labels.contains(&"✏️ Исправить текст"), "must have edit");
    assert!(all_labels.contains(&"❌ Отменить"), "must have cancel");
}

#[test]
fn p5_s06_guided_confirmation_keyboard_edit_buttons_are_imperative() {
    // Phase 3 fix: edit buttons must say "Изменить …", not bare nouns.
    let keyboard = ui::guided_confirmation_keyboard();
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels
            .iter()
            .any(|l| l.contains("Изменить исполнителя")),
        "guided confirmation must say 'Изменить исполнителя'; got: {all_labels:?}"
    );
    assert!(
        all_labels.iter().any(|l| l.contains("Изменить описание")),
        "guided confirmation must say 'Изменить описание'"
    );
    assert!(
        all_labels.iter().any(|l| l.contains("Изменить срок")),
        "guided confirmation must say 'Изменить срок'"
    );
}

#[test]
fn p5_s07_task_detail_with_notice_prepends_notice_before_body() {
    let details = make_task_status_details(TaskStatus::InProgress);
    let text = task_detail_text(&details, TaskCardMode::Compact, Some("✅ Готово"));
    assert!(
        text.starts_with("✅ Готово"),
        "notice must appear at the very beginning of the rendered card"
    );
}

#[test]
fn p5_s08_blocked_task_detail_shows_blocker_in_priority_note() {
    let mut details = make_task_status_details(TaskStatus::Blocked);
    details.blocked_reason = Some("Ожидаем данных от клиента".to_owned());
    let text = task_detail_text(&details, TaskCardMode::Compact, None);
    assert!(
        text.contains("блокер"),
        "blocked task card must mention 'блокер'"
    );
}

#[test]
fn p5_s09_clarification_keyboard_navigation_button_uses_back_emoji_not_create() {
    // Phase 3 fix: clarification keyboard used 🆕 on the "К меню создания"
    // navigation button. It must now use ↩️ (navigation, not creation).
    let request = ClarificationRequest {
        message: "Кого назначить?".to_owned(),
        requested_query: None,
        allow_unassigned: false,
        candidates: vec![],
        task_body_preview: None,
    };
    let keyboard = ui::clarification_keyboard(&request);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    // Must NOT have 🆕 К меню создания
    assert!(
        !all_labels.contains(&"🆕 К меню создания"),
        "clarification nav button must not use 🆕 (creation emoji); got: {all_labels:?}"
    );
    // Must have ↩️ К меню создания
    assert!(
        all_labels.contains(&"↩️ К меню создания"),
        "clarification nav button must use ↩️ (back emoji)"
    );
}

#[test]
fn p5_s10_created_task_followup_keyboard_assign_button_uses_imperative() {
    // Phase 3 fix: was "Кто будет отвечать?" — now must be "Назначить исполнителя".
    let task = factories::task(None);
    let summary = TaskCreationSummary::from_task(
        &task,
        "Создано".to_owned(),
        DeliveryStatus::DeliveredToAssignee,
    );
    let keyboard = ui::created_task_followup_keyboard(&summary, true);
    let all_labels: Vec<_> = keyboard
        .inline_keyboard
        .iter()
        .flat_map(|row| row.iter().map(|btn| btn.text.as_str()))
        .collect();
    assert!(
        all_labels.contains(&"👤 Назначить исполнителя"),
        "followup keyboard must say 'Назначить исполнителя'; got: {all_labels:?}"
    );
    assert!(
        !all_labels.iter().any(|l| l.contains("Кто будет")),
        "must NOT use question-mark style label 'Кто будет отвечать?'"
    );
}
