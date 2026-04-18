use uuid::Uuid;

use crate::domain::task::TaskStatus;

use super::types::{DraftEditField, TaskCardMode, TaskListOrigin, TelegramCallback};

const CALLBACK_GROUP_MENU: &str = "m";
const CALLBACK_GROUP_LIST: &str = "l";
const CALLBACK_GROUP_TASK: &str = "t";
const CALLBACK_GROUP_CREATE: &str = "c";
const CALLBACK_GROUP_DRAFT: &str = "d";
const CALLBACK_GROUP_INPUT: &str = "i";
const EMPTY_CURSOR: &str = "_";

pub fn encode_callback(callback: &TelegramCallback) -> String {
    match callback {
        TelegramCallback::MenuHome => format!("{CALLBACK_GROUP_MENU}:home"),
        TelegramCallback::MenuHelp => format!("{CALLBACK_GROUP_MENU}:help"),
        TelegramCallback::MenuSettings => format!("{CALLBACK_GROUP_MENU}:settings"),
        TelegramCallback::MenuStats => format!("{CALLBACK_GROUP_MENU}:stats"),
        TelegramCallback::MenuTeamStats => format!("{CALLBACK_GROUP_MENU}:team_stats"),
        TelegramCallback::MenuCreate => format!("{CALLBACK_GROUP_MENU}:create"),
        TelegramCallback::MenuSyncEmployees => format!("{CALLBACK_GROUP_MENU}:sync"),
        TelegramCallback::ListTasks { origin, cursor } => format!(
            "{CALLBACK_GROUP_LIST}:{}:{}",
            origin_code(*origin),
            cursor.as_deref().unwrap_or(EMPTY_CURSOR)
        ),
        TelegramCallback::OpenTask {
            task_uid,
            origin,
            mode,
        } => format!(
            "{CALLBACK_GROUP_TASK}:open:{}:{}:{}",
            origin_code(*origin),
            task_uid,
            task_card_mode_code(*mode)
        ),
        TelegramCallback::UpdateTaskStatus {
            task_uid,
            next_status,
            origin,
        } => format!(
            "{CALLBACK_GROUP_TASK}:status:{}:{}:{}",
            origin_code(*origin),
            task_uid,
            task_status_code(*next_status)
        ),
        TelegramCallback::ConfirmTaskCancel { task_uid, origin } => format!(
            "{CALLBACK_GROUP_TASK}:cancel_confirm:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::ExecuteTaskCancel { task_uid, origin } => format!(
            "{CALLBACK_GROUP_TASK}:cancel_execute:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::StartTaskCommentInput { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:comment:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::StartTaskBlockerInput { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:blocker:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::StartTaskReassignInput { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:reassign:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::ShowDeliveryHelp { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:delivery_help:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::StartQuickCreate => format!("{CALLBACK_GROUP_CREATE}:quick"),
        TelegramCallback::StartGuidedCreate => format!("{CALLBACK_GROUP_CREATE}:guided"),
        TelegramCallback::VoiceCreateConfirm => format!("{CALLBACK_GROUP_CREATE}:voice_confirm"),
        TelegramCallback::VoiceCreateEdit => format!("{CALLBACK_GROUP_CREATE}:voice_edit"),
        TelegramCallback::VoiceCreateBack => format!("{CALLBACK_GROUP_CREATE}:voice_back"),
        TelegramCallback::VoiceCreateCancel => format!("{CALLBACK_GROUP_CREATE}:voice_cancel"),
        TelegramCallback::RegistrationPickEmployee { employee_id } => {
            format!("{CALLBACK_GROUP_CREATE}:register_employee:{employee_id}")
        }
        TelegramCallback::RegistrationContinueUnlinked => {
            format!("{CALLBACK_GROUP_CREATE}:register_unlinked")
        }
        TelegramCallback::ClarificationPickEmployee { employee_id } => {
            format!("{CALLBACK_GROUP_CREATE}:clarify_employee:{employee_id}")
        }
        TelegramCallback::ClarificationCreateUnassigned => {
            format!("{CALLBACK_GROUP_CREATE}:clarify_unassigned")
        }
        TelegramCallback::DraftSkipAssignee => format!("{CALLBACK_GROUP_DRAFT}:skip_assignee"),
        TelegramCallback::DraftSkipDeadline => format!("{CALLBACK_GROUP_DRAFT}:skip_deadline"),
        TelegramCallback::DraftSubmit => format!("{CALLBACK_GROUP_DRAFT}:submit"),
        TelegramCallback::DraftEdit { field } => {
            format!("{CALLBACK_GROUP_DRAFT}:edit:{}", draft_field_code(*field))
        }
    }
}

pub fn parse_callback(value: &str) -> Option<TelegramCallback> {
    parse_legacy_callback(value).or_else(|| parse_callback_modern(value))
}

fn parse_callback_modern(value: &str) -> Option<TelegramCallback> {
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [CALLBACK_GROUP_MENU, "home"] => Some(TelegramCallback::MenuHome),
        [CALLBACK_GROUP_MENU, "help"] => Some(TelegramCallback::MenuHelp),
        [CALLBACK_GROUP_MENU, "settings"] => Some(TelegramCallback::MenuSettings),
        [CALLBACK_GROUP_MENU, "stats"] => Some(TelegramCallback::MenuStats),
        [CALLBACK_GROUP_MENU, "team_stats"] => Some(TelegramCallback::MenuTeamStats),
        [CALLBACK_GROUP_MENU, "create"] => Some(TelegramCallback::MenuCreate),
        [CALLBACK_GROUP_MENU, "sync"] => Some(TelegramCallback::MenuSyncEmployees),
        [CALLBACK_GROUP_LIST, scope, cursor] => Some(TelegramCallback::ListTasks {
            origin: parse_origin_code(scope)?,
            cursor: parse_cursor(cursor),
        }),
        [CALLBACK_GROUP_TASK, "open", origin, task_uid] => Some(TelegramCallback::OpenTask {
            origin: parse_origin_code(origin)?,
            task_uid: Uuid::parse_str(task_uid).ok()?,
            mode: TaskCardMode::Compact,
        }),
        [CALLBACK_GROUP_TASK, "open", origin, task_uid, mode] => Some(TelegramCallback::OpenTask {
            origin: parse_origin_code(origin)?,
            task_uid: Uuid::parse_str(task_uid).ok()?,
            mode: parse_task_card_mode(mode)?,
        }),
        [CALLBACK_GROUP_TASK, "status", origin, task_uid, status] => {
            Some(TelegramCallback::UpdateTaskStatus {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
                next_status: parse_task_status(status)?,
            })
        }
        [CALLBACK_GROUP_TASK, "cancel_confirm", origin, task_uid] => {
            Some(TelegramCallback::ConfirmTaskCancel {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_TASK, "cancel_execute", origin, task_uid] => {
            Some(TelegramCallback::ExecuteTaskCancel {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, "comment", origin, task_uid] => {
            Some(TelegramCallback::StartTaskCommentInput {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, "blocker", origin, task_uid] => {
            Some(TelegramCallback::StartTaskBlockerInput {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, "reassign", origin, task_uid] => {
            Some(TelegramCallback::StartTaskReassignInput {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, "delivery_help", origin, task_uid] => {
            Some(TelegramCallback::ShowDeliveryHelp {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_CREATE, "quick"] => Some(TelegramCallback::StartQuickCreate),
        [CALLBACK_GROUP_CREATE, "guided"] => Some(TelegramCallback::StartGuidedCreate),
        [CALLBACK_GROUP_CREATE, "voice_confirm"] => Some(TelegramCallback::VoiceCreateConfirm),
        [CALLBACK_GROUP_CREATE, "voice_edit"] => Some(TelegramCallback::VoiceCreateEdit),
        [CALLBACK_GROUP_CREATE, "voice_back"] => Some(TelegramCallback::VoiceCreateBack),
        [CALLBACK_GROUP_CREATE, "voice_cancel"] => Some(TelegramCallback::VoiceCreateCancel),
        [CALLBACK_GROUP_CREATE, "register_employee", employee_id] => {
            Some(TelegramCallback::RegistrationPickEmployee {
                employee_id: employee_id.parse::<i64>().ok()?,
            })
        }
        [CALLBACK_GROUP_CREATE, "register_unlinked"] => {
            Some(TelegramCallback::RegistrationContinueUnlinked)
        }
        [CALLBACK_GROUP_CREATE, "clarify_employee", employee_id] => {
            Some(TelegramCallback::ClarificationPickEmployee {
                employee_id: employee_id.parse::<i64>().ok()?,
            })
        }
        [CALLBACK_GROUP_CREATE, "clarify_unassigned"] => {
            Some(TelegramCallback::ClarificationCreateUnassigned)
        }
        [CALLBACK_GROUP_DRAFT, "skip_assignee"] => Some(TelegramCallback::DraftSkipAssignee),
        [CALLBACK_GROUP_DRAFT, "skip_deadline"] => Some(TelegramCallback::DraftSkipDeadline),
        [CALLBACK_GROUP_DRAFT, "submit"] => Some(TelegramCallback::DraftSubmit),
        [CALLBACK_GROUP_DRAFT, "edit", field] => Some(TelegramCallback::DraftEdit {
            field: parse_draft_field(field)?,
        }),
        _ => None,
    }
}

fn parse_legacy_callback(value: &str) -> Option<TelegramCallback> {
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        ["status", task_uid, status] => Some(TelegramCallback::UpdateTaskStatus {
            task_uid: Uuid::parse_str(task_uid).ok()?,
            next_status: parse_task_status(status)?,
            origin: TaskListOrigin::Assigned,
        }),
        ["open", task_uid] => Some(TelegramCallback::OpenTask {
            task_uid: Uuid::parse_str(task_uid).ok()?,
            origin: TaskListOrigin::Assigned,
            mode: TaskCardMode::Compact,
        }),
        ["block", task_uid] => Some(TelegramCallback::StartTaskBlockerInput {
            task_uid: Uuid::parse_str(task_uid).ok()?,
            origin: TaskListOrigin::Assigned,
        }),
        _ => None,
    }
}

fn origin_code(origin: TaskListOrigin) -> &'static str {
    match origin {
        TaskListOrigin::Assigned => "assigned",
        TaskListOrigin::Created => "created",
        TaskListOrigin::Team => "team",
        TaskListOrigin::Focus => "focus",
        TaskListOrigin::ManagerInbox => "manager_inbox",
    }
}

fn parse_origin_code(value: &str) -> Option<TaskListOrigin> {
    match value {
        "assigned" => Some(TaskListOrigin::Assigned),
        "created" => Some(TaskListOrigin::Created),
        "team" => Some(TaskListOrigin::Team),
        "focus" => Some(TaskListOrigin::Focus),
        "manager_inbox" => Some(TaskListOrigin::ManagerInbox),
        _ => None,
    }
}

fn task_card_mode_code(mode: TaskCardMode) -> &'static str {
    match mode {
        TaskCardMode::Compact => "compact",
        TaskCardMode::Expanded => "expanded",
    }
}

fn parse_task_card_mode(value: &str) -> Option<TaskCardMode> {
    match value {
        "compact" => Some(TaskCardMode::Compact),
        "expanded" => Some(TaskCardMode::Expanded),
        _ => None,
    }
}

fn task_status_code(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Created => "created",
        TaskStatus::Sent => "sent",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Blocked => "blocked",
        TaskStatus::InReview => "in_review",
        TaskStatus::Completed => "completed",
        TaskStatus::Cancelled => "cancelled",
    }
}

fn parse_task_status(value: &str) -> Option<TaskStatus> {
    match value {
        "created" => Some(TaskStatus::Created),
        "sent" => Some(TaskStatus::Sent),
        "in_progress" => Some(TaskStatus::InProgress),
        "blocked" => Some(TaskStatus::Blocked),
        "in_review" => Some(TaskStatus::InReview),
        "completed" => Some(TaskStatus::Completed),
        "cancelled" => Some(TaskStatus::Cancelled),
        _ => None,
    }
}

fn parse_cursor(value: &str) -> Option<String> {
    if value == EMPTY_CURSOR {
        None
    } else {
        Some(value.to_owned())
    }
}

fn draft_field_code(field: DraftEditField) -> &'static str {
    match field {
        DraftEditField::Assignee => "assignee",
        DraftEditField::Description => "description",
        DraftEditField::Deadline => "deadline",
    }
}

fn parse_draft_field(value: &str) -> Option<DraftEditField> {
    match value {
        "assignee" => Some(DraftEditField::Assignee),
        "description" => Some(DraftEditField::Description),
        "deadline" => Some(DraftEditField::Deadline),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::{encode_callback, parse_callback};
    use crate::domain::task::TaskStatus;
    use crate::presentation::telegram::callbacks::{
        TaskCardMode, TaskListOrigin, TelegramCallback,
    };

    #[test]
    fn given_modern_status_callback_when_parse_then_roundtrip_succeeds() {
        let task_uid = Uuid::now_v7();
        let callback = TelegramCallback::UpdateTaskStatus {
            task_uid,
            next_status: TaskStatus::InReview,
            origin: TaskListOrigin::Created,
        };

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_legacy_status_callback_when_parse_then_assigns_default_origin() {
        let task_uid = Uuid::now_v7();
        let encoded = format!("status:{task_uid}:in_progress");

        let parsed = parse_callback(&encoded);

        assert_eq!(
            parsed,
            Some(TelegramCallback::UpdateTaskStatus {
                task_uid,
                next_status: TaskStatus::InProgress,
                origin: TaskListOrigin::Assigned,
            })
        );
    }

    #[test]
    fn given_legacy_open_callback_when_parse_then_builds_open_action() {
        let task_uid = Uuid::now_v7();
        let encoded = format!("open:{task_uid}");

        let parsed = parse_callback(&encoded);

        assert_eq!(
            parsed,
            Some(TelegramCallback::OpenTask {
                task_uid,
                origin: TaskListOrigin::Assigned,
                mode: TaskCardMode::Compact,
            })
        );
    }

    #[test]
    fn given_voice_confirm_callback_when_roundtrip_then_it_parses_back() {
        let callback = TelegramCallback::VoiceCreateConfirm;

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_delivery_help_callback_when_roundtrip_then_it_parses_back() {
        let task_uid = Uuid::now_v7();
        let callback = TelegramCallback::ShowDeliveryHelp {
            task_uid,
            origin: TaskListOrigin::Created,
        };

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_clarification_employee_callback_when_roundtrip_then_it_parses_back() {
        let callback = TelegramCallback::ClarificationPickEmployee { employee_id: 42 };

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_registration_employee_callback_when_roundtrip_then_it_parses_back() {
        let callback = TelegramCallback::RegistrationPickEmployee { employee_id: 5 };

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }
}
