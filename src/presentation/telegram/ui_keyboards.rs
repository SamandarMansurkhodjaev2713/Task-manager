use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use super::ui_shared::{
    action_label, back_label, is_dangerous_action, next_best_action, status_badge, truncate_title,
};
use crate::application::dto::task_views::{
    DeliveryStatus, EmployeeCandidateView, TaskActionView, TaskCreationOutcome, TaskListItem,
    TaskListPage, TaskStatusDetails,
};
use crate::domain::user::{User, UserRole};
use crate::presentation::telegram::admin_nonce_store::PendingAdminAction;
use crate::presentation::telegram::callbacks::{
    action_to_status, encode_callback, AdminRoleOption, DraftEditField, TaskCardMode,
    TaskListOrigin, TelegramCallback,
};

pub fn main_menu_keyboard(actor: &User) -> InlineKeyboardMarkup {
    let mut rows = vec![
        vec![
            button("🧭 Мой фокус", list_callback(TaskListOrigin::Focus, None)),
            button("🆕 Создать задачу", TelegramCallback::MenuCreate),
        ],
        vec![
            button(
                "📥 Мои задачи",
                list_callback(TaskListOrigin::Assigned, None),
            ),
            button(
                "📤 Созданные мной",
                list_callback(TaskListOrigin::Created, None),
            ),
        ],
        vec![
            button("📊 Моя статистика", TelegramCallback::MenuStats),
            button("⚙️ Профиль", TelegramCallback::MenuSettings),
        ],
        vec![button("❓ Помощь", TelegramCallback::MenuHelp)],
    ];

    if actor.role.is_manager_or_admin() {
        rows.insert(
            2,
            vec![
                button(
                    "👥 Командные задачи",
                    list_callback(TaskListOrigin::Team, None),
                ),
                button(
                    "🧪 Inbox менеджера",
                    list_callback(TaskListOrigin::ManagerInbox, None),
                ),
            ],
        );
        rows.insert(
            3,
            vec![button(
                "📈 Командная статистика",
                TelegramCallback::MenuTeamStats,
            )],
        );
    }

    if actor.role.is_admin() {
        rows.push(vec![button(
            "🔄 Синхронизировать сотрудников",
            TelegramCallback::MenuSyncEmployees,
        )]);
    }

    InlineKeyboardMarkup::new(rows)
}

pub fn create_menu_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            button("⚡ Быстрый режим", TelegramCallback::StartQuickCreate),
            button("🧭 Пошагово", TelegramCallback::StartGuidedCreate),
        ],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

/// Keyboard shown during the quick-capture session.
/// Intentionally minimal — the user is already in capture mode; mode-switch buttons
/// belong on CreateMenu, not on QuickCreate, where their `ScreenDescriptor` would
/// mismatch and produce a confusing "экран устарел" toast.
pub fn quick_capture_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![button("🏠 В меню", TelegramCallback::MenuHome)]])
}

pub fn task_list_keyboard(origin: TaskListOrigin, page: &TaskListPage) -> InlineKeyboardMarkup {
    let mut rows = page
        .sections
        .iter()
        .flat_map(|section| section.tasks.iter())
        .map(|task| {
            vec![button(
                &task_list_button_label(task),
                TelegramCallback::OpenTask {
                    task_uid: task.task_uid,
                    origin,
                    mode: TaskCardMode::Compact,
                },
            )]
        })
        .collect::<Vec<_>>();

    if let Some(cursor) = &page.next_cursor {
        rows.push(vec![button(
            "Ещё задачи",
            TelegramCallback::ListTasks {
                origin,
                cursor: Some(cursor.clone()),
            },
        )]);
    }

    rows.push(vec![button("🏠 В меню", TelegramCallback::MenuHome)]);
    InlineKeyboardMarkup::new(rows)
}

pub fn task_detail_keyboard(
    details: &TaskStatusDetails,
    origin: TaskListOrigin,
    mode: TaskCardMode,
) -> InlineKeyboardMarkup {
    let primary_action = next_best_action(&details.available_actions);
    let mut rows = Vec::new();

    if let Some(action) = primary_action {
        rows.push(vec![action_button(action, details, origin)]);
    }

    let secondary_actions = details
        .available_actions
        .iter()
        .copied()
        .filter(|action| Some(*action) != primary_action)
        .filter(|action| !is_dangerous_action(*action))
        .map(|action| action_button(action, details, origin))
        .collect::<Vec<_>>();
    for chunk in secondary_actions.chunks(2) {
        rows.push(chunk.to_vec());
    }

    let dangerous_actions = details
        .available_actions
        .iter()
        .copied()
        .filter(|action| is_dangerous_action(*action))
        .map(|action| action_button(action, details, origin))
        .collect::<Vec<_>>();
    if !dangerous_actions.is_empty() {
        rows.push(dangerous_actions);
    }

    if details.delivery_status == Some(DeliveryStatus::PendingAssigneeRegistration) {
        rows.push(vec![button(
            "👋 Как подключить исполнителя",
            TelegramCallback::ShowDeliveryHelp {
                task_uid: details.task_uid,
                origin,
            },
        )]);
    }

    rows.push(vec![task_view_toggle_button(details, origin, mode)]);
    rows.push(vec![button(back_label(origin), back_callback(origin))]);
    rows.push(vec![button("🏠 В меню", TelegramCallback::MenuHome)]);
    InlineKeyboardMarkup::new(rows)
}

pub fn cancel_confirmation_keyboard(
    task_uid: uuid::Uuid,
    origin: TaskListOrigin,
) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            button(
                "✅ Да, отменить",
                TelegramCallback::ExecuteTaskCancel { task_uid, origin },
            ),
            button(
                "↩️ Назад",
                TelegramCallback::OpenTask {
                    task_uid,
                    origin,
                    mode: TaskCardMode::Compact,
                },
            ),
        ],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

pub fn delivery_help_keyboard(
    task_uid: uuid::Uuid,
    origin: TaskListOrigin,
) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button(
            "↩️ Вернуться к задаче",
            TelegramCallback::OpenTask {
                task_uid,
                origin,
                mode: TaskCardMode::Compact,
            },
        )],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

pub fn outcome_keyboard(outcome: &TaskCreationOutcome) -> InlineKeyboardMarkup {
    match outcome {
        TaskCreationOutcome::Created(summary) | TaskCreationOutcome::DuplicateFound(summary) => {
            InlineKeyboardMarkup::new(vec![
                vec![button(
                    "📋 Открыть карточку",
                    TelegramCallback::OpenTask {
                        task_uid: summary.task_uid,
                        origin: TaskListOrigin::Created,
                        mode: TaskCardMode::Compact,
                    },
                )],
                vec![
                    button("🆕 Ещё задача", TelegramCallback::MenuCreate),
                    button("🏠 В меню", TelegramCallback::MenuHome),
                ],
            ])
        }
        TaskCreationOutcome::ClarificationRequired(_) => InlineKeyboardMarkup::new(vec![
            vec![button(
                "🆕 Уточнить и создать",
                TelegramCallback::MenuCreate,
            )],
            vec![button("🏠 В меню", TelegramCallback::MenuHome)],
        ]),
    }
}

pub fn guided_assignee_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button(
            "Без исполнителя",
            TelegramCallback::DraftSkipAssignee,
        )],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

/// Suggestion keyboard shown during the guided Assignee step when the entered
/// text produces ambiguous or partial-confidence matches.
///
/// Each candidate gets its own button.  The "Без исполнителя" escape and the
/// home button are always appended so the user is never trapped.
pub fn guided_assignee_suggestions_keyboard(
    candidates: &[crate::application::dto::task_views::EmployeeCandidateView],
) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<teloxide::types::InlineKeyboardButton>> = candidates
        .iter()
        .filter_map(|candidate| {
            candidate.employee_id.map(|employee_id| {
                // Show username in parentheses when available so the user
                // can cross-reference against their Telegram contacts.
                let label = match candidate.telegram_username.as_deref() {
                    Some(username) => format!("{} (@{})", candidate.full_name, username),
                    None => candidate.full_name.clone(),
                };
                vec![button(
                    &label,
                    TelegramCallback::GuidedAssigneeConfirm { employee_id },
                )]
            })
        })
        .collect();

    rows.push(vec![button(
        "Без исполнителя",
        TelegramCallback::DraftSkipAssignee,
    )]);
    rows.push(vec![button("🏠 В меню", TelegramCallback::MenuHome)]);

    InlineKeyboardMarkup::new(rows)
}

pub fn guided_deadline_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button("Без срока", TelegramCallback::DraftSkipDeadline)],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

pub fn guided_confirmation_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button("✅ Создать задачу", TelegramCallback::DraftSubmit)],
        vec![
            button(
                "👤 Исполнитель",
                TelegramCallback::DraftEdit {
                    field: DraftEditField::Assignee,
                },
            ),
            button(
                "📝 Описание",
                TelegramCallback::DraftEdit {
                    field: DraftEditField::Description,
                },
            ),
        ],
        vec![button(
            "⏰ Срок",
            TelegramCallback::DraftEdit {
                field: DraftEditField::Deadline,
            },
        )],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

pub fn voice_confirmation_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button(
            "✅ Создать задачу",
            TelegramCallback::VoiceCreateConfirm,
        )],
        vec![
            button("✏️ Исправить текст", TelegramCallback::VoiceCreateEdit),
            button("❌ Отменить", TelegramCallback::VoiceCreateCancel),
        ],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

pub fn voice_edit_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            button("↩️ Назад к расшифровке", TelegramCallback::VoiceCreateBack),
            button("❌ Отменить", TelegramCallback::VoiceCreateCancel),
        ],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

fn task_list_button_label(task: &TaskListItem) -> String {
    let deadline = task
        .deadline
        .map(|value| format!(" • {}", value.format("%d.%m")))
        .unwrap_or_default();
    let title = truncate_title(&task.title);
    format!(
        "{} {} {}{}",
        task.public_code,
        status_badge(task.status),
        title,
        deadline
    )
}

fn action_button(
    action: TaskActionView,
    details: &TaskStatusDetails,
    origin: TaskListOrigin,
) -> InlineKeyboardButton {
    match action {
        TaskActionView::Cancel => button(
            action_label(action),
            TelegramCallback::ConfirmTaskCancel {
                task_uid: details.task_uid,
                origin,
            },
        ),
        TaskActionView::AddComment => button(
            action_label(action),
            TelegramCallback::StartTaskCommentInput {
                task_uid: details.task_uid,
                origin,
            },
        ),
        TaskActionView::ReportBlocker => button(
            action_label(action),
            TelegramCallback::StartTaskBlockerInput {
                task_uid: details.task_uid,
                origin,
            },
        ),
        TaskActionView::Reassign => button(
            action_label(action),
            TelegramCallback::StartTaskReassignInput {
                task_uid: details.task_uid,
                origin,
            },
        ),
        _ => {
            let next_status = action_to_status(action)
                .expect("status transition action must resolve to a concrete task status");
            button(
                action_label(action),
                TelegramCallback::UpdateTaskStatus {
                    task_uid: details.task_uid,
                    next_status,
                    origin,
                },
            )
        }
    }
}

fn task_view_toggle_button(
    details: &TaskStatusDetails,
    origin: TaskListOrigin,
    mode: TaskCardMode,
) -> InlineKeyboardButton {
    match mode {
        TaskCardMode::Compact => button(
            "🔎 Подробнее",
            TelegramCallback::OpenTask {
                task_uid: details.task_uid,
                origin,
                mode: TaskCardMode::Expanded,
            },
        ),
        TaskCardMode::Expanded => button(
            "🪶 Коротко",
            TelegramCallback::OpenTask {
                task_uid: details.task_uid,
                origin,
                mode: TaskCardMode::Compact,
            },
        ),
    }
}

fn list_callback(origin: TaskListOrigin, cursor: Option<String>) -> TelegramCallback {
    TelegramCallback::ListTasks { origin, cursor }
}

fn back_callback(origin: TaskListOrigin) -> TelegramCallback {
    TelegramCallback::ListTasks {
        origin,
        cursor: None,
    }
}

fn button(text: &str, callback: TelegramCallback) -> InlineKeyboardButton {
    InlineKeyboardButton::callback(text.to_owned(), encode_callback(&callback))
}

pub fn clarification_keyboard(
    request: &crate::application::dto::task_views::ClarificationRequest,
) -> InlineKeyboardMarkup {
    let mut rows = request
        .candidates
        .iter()
        .filter_map(|candidate| {
            candidate.employee_id.map(|employee_id| {
                vec![button(
                    &clarification_candidate_label(candidate),
                    TelegramCallback::ClarificationPickEmployee { employee_id },
                )]
            })
        })
        .collect::<Vec<_>>();

    if request.allow_unassigned {
        rows.push(vec![button(
            "Создать без исполнителя",
            TelegramCallback::ClarificationCreateUnassigned,
        )]);
    }

    rows.push(vec![button(
        "🆕 К меню создания",
        TelegramCallback::MenuCreate,
    )]);
    rows.push(vec![button("🏠 В меню", TelegramCallback::MenuHome)]);

    InlineKeyboardMarkup::new(rows)
}

pub fn registration_link_keyboard(
    candidates: &[EmployeeCandidateView],
    allow_continue_unlinked: bool,
) -> InlineKeyboardMarkup {
    let mut rows = candidates
        .iter()
        .filter_map(|candidate| {
            candidate.employee_id.map(|employee_id| {
                vec![button(
                    &clarification_candidate_label(candidate),
                    TelegramCallback::RegistrationPickEmployee { employee_id },
                )]
            })
        })
        .collect::<Vec<_>>();

    if allow_continue_unlinked {
        rows.push(vec![button(
            "Продолжить без привязки",
            TelegramCallback::RegistrationContinueUnlinked,
        )]);
    }

    InlineKeyboardMarkup::new(rows)
}

pub fn created_task_followup_keyboard(
    summary: &crate::application::dto::task_views::TaskCreationSummary,
    allow_assign_owner: bool,
) -> InlineKeyboardMarkup {
    let mut rows = vec![vec![button(
        "📋 Открыть карточку",
        TelegramCallback::OpenTask {
            task_uid: summary.task_uid,
            origin: TaskListOrigin::Created,
            mode: TaskCardMode::Compact,
        },
    )]];

    if allow_assign_owner {
        rows.push(vec![button(
            "👤 Кто будет отвечать?",
            TelegramCallback::StartTaskReassignInput {
                task_uid: summary.task_uid,
                origin: TaskListOrigin::Created,
            },
        )]);
    }

    rows.push(vec![
        button("🆕 Ещё задача", TelegramCallback::MenuCreate),
        button("🏠 В меню", TelegramCallback::MenuHome),
    ]);

    InlineKeyboardMarkup::new(rows)
}

fn clarification_candidate_label(candidate: &EmployeeCandidateView) -> String {
    let username = candidate
        .telegram_username
        .as_ref()
        .map(|value| format!(" (@{value})"))
        .unwrap_or_default();

    format!("{}{}", candidate.full_name, username)
}

// ── Admin-panel keyboards (Phase 4) ──────────────────────────────────────

/// Top-level admin-panel menu.  We intentionally keep only a small number of
/// options here so the admin can reach anything in at most two taps.
pub fn admin_menu_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button("👥 Администраторы", TelegramCallback::AdminUsers)],
        vec![
            button("📜 Журнал действий", TelegramCallback::AdminAudit),
            button("🔐 Безопасность", TelegramCallback::AdminSecurityAudit),
        ],
        vec![button("🚩 Флаги функций", TelegramCallback::AdminFeatures)],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

/// Lists the currently active admins.  Each row opens the user's detail
/// card — role changes and deactivate/reactivate live there to avoid
/// overflowing the screen.  If the actor is viewing the list, they see
/// their own row but cannot click into it (the detail-card callback still
/// hits the self-target guard).
pub fn admin_users_keyboard(users: &[User]) -> InlineKeyboardMarkup {
    let mut rows = users
        .iter()
        .filter_map(|user| {
            user.id.map(|user_id| {
                vec![button(
                    &admin_user_list_label(user),
                    TelegramCallback::AdminUserDetails { user_id },
                )]
            })
        })
        .collect::<Vec<_>>();

    rows.push(vec![button("↩️ В панель", TelegramCallback::AdminMenu)]);
    rows.push(vec![button("🏠 В меню", TelegramCallback::MenuHome)]);

    InlineKeyboardMarkup::new(rows)
}

/// User detail card — shows role-change buttons, deactivate / reactivate,
/// and navigation.  We hide role buttons for the current role so the admin
/// can't accidentally submit a "no-op" action (which would still cost them
/// a nonce round-trip).
pub fn admin_user_details_keyboard(target: &User) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    let user_id = match target.id {
        Some(id) => id,
        None => {
            rows.push(vec![button("↩️ В панель", TelegramCallback::AdminMenu)]);
            return InlineKeyboardMarkup::new(rows);
        }
    };

    let mut role_row = Vec::new();
    for role in [
        (AdminRoleOption::User, UserRole::User, "👤 Пользователь"),
        (AdminRoleOption::Manager, UserRole::Manager, "🧭 Менеджер"),
        (AdminRoleOption::Admin, UserRole::Admin, "🛡 Админ"),
    ] {
        let (option, domain_role, label) = role;
        if target.role == domain_role {
            continue;
        }
        role_row.push(button(
            label,
            TelegramCallback::AdminUserPrepareRoleChange {
                user_id,
                next_role: option,
            },
        ));
    }
    if !role_row.is_empty() {
        rows.push(role_row);
    }

    if target.deactivated_at.is_some() {
        rows.push(vec![button(
            "✅ Активировать",
            TelegramCallback::AdminUserPrepareReactivate { user_id },
        )]);
    } else {
        rows.push(vec![button(
            "⛔ Деактивировать",
            TelegramCallback::AdminUserPrepareDeactivate { user_id },
        )]);
    }

    rows.push(vec![button("👥 К списку", TelegramCallback::AdminUsers)]);
    rows.push(vec![button("↩️ В панель", TelegramCallback::AdminMenu)]);
    rows.push(vec![button("🏠 В меню", TelegramCallback::MenuHome)]);

    InlineKeyboardMarkup::new(rows)
}

/// Two-step confirmation keyboard for destructive admin actions.  The nonce
/// is baked into the "Confirm" button so the underlying action is
/// tamper-proof — the user can't swap `user_id` before confirming.
pub fn admin_confirmation_keyboard(nonce: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            button(
                "✅ Подтвердить",
                TelegramCallback::AdminConfirmNonce {
                    nonce: nonce.to_owned(),
                },
            ),
            button("❌ Отмена", TelegramCallback::AdminCancelPending),
        ],
        vec![button("↩️ В панель", TelegramCallback::AdminMenu)],
    ])
}

/// Simple back-to-menu keyboard shown at the end of transient admin
/// screens (audit log, etc.).
pub fn admin_back_keyboard() -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![button("↩️ В панель", TelegramCallback::AdminMenu)],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

/// Feature flag management keyboard.  Shows one toggle row per flag
/// with a ✅/⬜ indicator and a button to flip the state; the last two
/// rows are navigation anchors.
///
/// `flags` must be `all_flags()` from the live [`FeatureFlagRegistry`].
pub fn admin_features_keyboard(
    flags: &[(crate::shared::feature_flags::FeatureFlag, bool)],
) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = flags
        .iter()
        .map(|(flag, enabled)| {
            let label = if *enabled {
                format!("✅ {} (откл.)", flag.as_key())
            } else {
                format!("⬜ {} (вкл.)", flag.as_key())
            };
            vec![button(
                &label,
                TelegramCallback::AdminToggleFeature {
                    flag_key: flag.as_key().to_owned(),
                },
            )]
        })
        .collect();
    rows.push(vec![button("↩️ В панель", TelegramCallback::AdminMenu)]);
    rows.push(vec![button("🏠 В меню", TelegramCallback::MenuHome)]);
    InlineKeyboardMarkup::new(rows)
}

fn admin_user_list_label(user: &User) -> String {
    let display = user.display_name_object();
    let name = if matches!(
        display.kind(),
        crate::domain::user::DisplayNameKind::Anonymous
    ) {
        format!("ID {}", user.telegram_id)
    } else {
        display.as_str().to_owned()
    };

    let role_badge = match user.role {
        UserRole::Admin => "🛡",
        UserRole::Manager => "🧭",
        UserRole::User => "👤",
    };
    let deact = if user.deactivated_at.is_some() {
        " • ⛔"
    } else {
        ""
    };
    format!("{role_badge} {name}{deact}")
}

/// Human-readable summary of a pending admin action, used inside the
/// confirmation screen.  Extracted here so the dispatcher can format the
/// message and keyboard in one place.
pub fn describe_pending_admin_action(action: &PendingAdminAction) -> String {
    match action {
        PendingAdminAction::ChangeRole {
            display_name,
            next_role,
            target_telegram_id,
            ..
        } => {
            let role_label = match next_role {
                AdminRoleOption::User => "пользователь",
                AdminRoleOption::Manager => "менеджер",
                AdminRoleOption::Admin => "админ",
            };
            format!(
                "Назначить пользователю {display_name} (tg id {target_telegram_id}) роль: {role_label}"
            )
        }
        PendingAdminAction::Deactivate {
            display_name,
            target_telegram_id,
            ..
        } => format!(
            "Деактивировать пользователя {display_name} (tg id {target_telegram_id}).\n\
             Он потеряет доступ к боту до ручной реактивации."
        ),
        PendingAdminAction::Reactivate {
            display_name,
            target_telegram_id,
            ..
        } => format!("Реактивировать пользователя {display_name} (tg id {target_telegram_id})."),
    }
}
