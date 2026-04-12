use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

use crate::application::dto::task_views::{
    TaskActionView, TaskCreationOutcome, TaskListPage, TaskStatusDetails,
};
use crate::domain::user::User;
use crate::presentation::telegram::callbacks::{
    action_to_status, encode_callback, DraftEditField, TaskListOrigin, TelegramCallback,
};
use crate::presentation::telegram::ui_shared::{
    action_label, back_label, status_badge, truncate_title,
};

pub fn main_menu_keyboard(actor: &User) -> InlineKeyboardMarkup {
    let mut rows = vec![
        vec![button("🆕 Создать задачу", TelegramCallback::MenuCreate)],
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
                button("📈 Командная статистика", TelegramCallback::MenuTeamStats),
            ],
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

pub fn task_list_keyboard(origin: TaskListOrigin, page: &TaskListPage) -> InlineKeyboardMarkup {
    let mut rows = page
        .sections
        .iter()
        .flat_map(|section| section.tasks.iter())
        .map(|task| {
            let label = format!(
                "{} {}",
                status_badge(&task.status.to_string()),
                truncate_title(&task.title)
            );
            vec![button(
                &label,
                TelegramCallback::OpenTask {
                    task_uid: task.task_uid,
                    origin,
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
) -> InlineKeyboardMarkup {
    let mut action_buttons = Vec::new();

    for action in &details.available_actions {
        match action {
            TaskActionView::Cancel => action_buttons.push(button(
                action_label(*action),
                TelegramCallback::ConfirmTaskCancel {
                    task_uid: details.task_uid,
                    origin,
                },
            )),
            TaskActionView::AddComment => action_buttons.push(button(
                action_label(*action),
                TelegramCallback::StartTaskCommentInput {
                    task_uid: details.task_uid,
                    origin,
                },
            )),
            TaskActionView::ReportBlocker => action_buttons.push(button(
                action_label(*action),
                TelegramCallback::StartTaskBlockerInput {
                    task_uid: details.task_uid,
                    origin,
                },
            )),
            TaskActionView::Reassign => action_buttons.push(button(
                action_label(*action),
                TelegramCallback::StartTaskReassignInput {
                    task_uid: details.task_uid,
                    origin,
                },
            )),
            _ => {
                if let Some(status) = action_to_status(*action) {
                    action_buttons.push(button(
                        action_label(*action),
                        TelegramCallback::UpdateTaskStatus {
                            task_uid: details.task_uid,
                            next_status: status,
                            origin,
                        },
                    ));
                }
            }
        }
    }

    let mut rows = action_buttons
        .chunks(2)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
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
            button("↩️ Назад", TelegramCallback::OpenTask { task_uid, origin }),
        ],
        vec![button("🏠 В меню", TelegramCallback::MenuHome)],
    ])
}

pub fn outcome_keyboard(outcome: &TaskCreationOutcome) -> InlineKeyboardMarkup {
    match outcome {
        TaskCreationOutcome::Created(summary) => InlineKeyboardMarkup::new(vec![
            vec![button(
                "📋 Открыть карточку",
                TelegramCallback::OpenTask {
                    task_uid: summary.task_uid,
                    origin: TaskListOrigin::Created,
                },
            )],
            vec![
                button("🆕 Ещё задача", TelegramCallback::MenuCreate),
                button("🏠 В меню", TelegramCallback::MenuHome),
            ],
        ]),
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
