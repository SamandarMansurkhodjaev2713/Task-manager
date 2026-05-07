use uuid::Uuid;

use crate::domain::task::TaskStatus;

use super::types::{
    AdminRoleOption, DraftEditField, HelpSection, TaskCardMode, TaskListOrigin, TelegramCallback,
};

const CALLBACK_GROUP_MENU: &str = "m";
const CALLBACK_GROUP_LIST: &str = "l";
const CALLBACK_GROUP_TASK: &str = "t";
const CALLBACK_GROUP_CREATE: &str = "c";
const CALLBACK_GROUP_DRAFT: &str = "d";
const CALLBACK_GROUP_INPUT: &str = "i";
const CALLBACK_GROUP_ADMIN: &str = "a";
const EMPTY_CURSOR: &str = "_";
const TELEGRAM_CALLBACK_DATA_MAX_BYTES: usize = 64;

pub fn encode_callback(callback: &TelegramCallback) -> String {
    let encoded = match callback {
        TelegramCallback::MenuHome => format!("{CALLBACK_GROUP_MENU}:home"),
        TelegramCallback::MenuHelp => format!("{CALLBACK_GROUP_MENU}:help"),
        TelegramCallback::MenuHelpSection { section } => {
            format!("{CALLBACK_GROUP_MENU}:hsec:{}", section.as_code())
        }
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
            "{CALLBACK_GROUP_TASK}:o:{}:{}:{}",
            origin_code(*origin),
            task_uid,
            task_card_mode_code(*mode)
        ),
        TelegramCallback::UpdateTaskStatus {
            task_uid,
            next_status,
            origin,
        } => format!(
            "{CALLBACK_GROUP_TASK}:s:{}:{}:{}",
            origin_code(*origin),
            task_uid,
            task_status_code(*next_status)
        ),
        TelegramCallback::ConfirmTaskCancel { task_uid, origin } => format!(
            "{CALLBACK_GROUP_TASK}:cc:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::ExecuteTaskCancel { task_uid, origin } => format!(
            "{CALLBACK_GROUP_TASK}:cx:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::StartTaskCommentInput { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:cm:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::StartTaskBlockerInput { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:b:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::StartTaskReassignInput { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:r:{}:{}",
            origin_code(*origin),
            task_uid
        ),
        TelegramCallback::ShowDeliveryHelp { task_uid, origin } => format!(
            "{CALLBACK_GROUP_INPUT}:dh:{}:{}",
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
        TelegramCallback::GuidedAssigneeConfirm { employee_id } => {
            format!("{CALLBACK_GROUP_DRAFT}:assignee_pick:{employee_id}")
        }
        TelegramCallback::AdminMenu => format!("{CALLBACK_GROUP_ADMIN}:menu"),
        TelegramCallback::AdminUsers => format!("{CALLBACK_GROUP_ADMIN}:users"),
        TelegramCallback::AdminUserDetails { user_id } => {
            format!("{CALLBACK_GROUP_ADMIN}:user:{user_id}")
        }
        TelegramCallback::AdminUserPrepareRoleChange { user_id, next_role } => format!(
            "{CALLBACK_GROUP_ADMIN}:role:{user_id}:{}",
            next_role.as_code()
        ),
        TelegramCallback::AdminUserPrepareDeactivate { user_id } => {
            format!("{CALLBACK_GROUP_ADMIN}:deact:{user_id}")
        }
        TelegramCallback::AdminUserPrepareReactivate { user_id } => {
            format!("{CALLBACK_GROUP_ADMIN}:react:{user_id}")
        }
        TelegramCallback::AdminConfirmNonce { nonce } => {
            format!("{CALLBACK_GROUP_ADMIN}:confirm:{nonce}")
        }
        TelegramCallback::AdminCancelPending => format!("{CALLBACK_GROUP_ADMIN}:cancel"),
        TelegramCallback::AdminAudit => format!("{CALLBACK_GROUP_ADMIN}:audit"),
        TelegramCallback::AdminSecurityAudit => format!("{CALLBACK_GROUP_ADMIN}:sec_audit"),
        TelegramCallback::AdminFeatures => format!("{CALLBACK_GROUP_ADMIN}:feat"),
        TelegramCallback::AdminToggleFeature { flag_key } => {
            format!("{CALLBACK_GROUP_ADMIN}:ftog:{flag_key}")
        }
    };
    debug_assert!(
        encoded.len() <= TELEGRAM_CALLBACK_DATA_MAX_BYTES,
        "Telegram callback_data exceeds {TELEGRAM_CALLBACK_DATA_MAX_BYTES} bytes: {encoded}"
    );
    encoded
}

pub fn parse_callback(value: &str) -> Option<TelegramCallback> {
    parse_legacy_callback(value).or_else(|| parse_callback_modern(value))
}

fn parse_callback_modern(value: &str) -> Option<TelegramCallback> {
    let parts = value.split(':').collect::<Vec<_>>();
    match parts.as_slice() {
        [CALLBACK_GROUP_MENU, "home"] => Some(TelegramCallback::MenuHome),
        [CALLBACK_GROUP_MENU, "help"] => Some(TelegramCallback::MenuHelp),
        [CALLBACK_GROUP_MENU, "hsec", code] => Some(TelegramCallback::MenuHelpSection {
            section: HelpSection::from_code(code)?,
        }),
        [CALLBACK_GROUP_MENU, "settings"] => Some(TelegramCallback::MenuSettings),
        [CALLBACK_GROUP_MENU, "stats"] => Some(TelegramCallback::MenuStats),
        [CALLBACK_GROUP_MENU, "team_stats"] => Some(TelegramCallback::MenuTeamStats),
        [CALLBACK_GROUP_MENU, "create"] => Some(TelegramCallback::MenuCreate),
        [CALLBACK_GROUP_MENU, "sync"] => Some(TelegramCallback::MenuSyncEmployees),
        [CALLBACK_GROUP_LIST, scope, cursor] => Some(TelegramCallback::ListTasks {
            origin: parse_origin_code(scope)?,
            cursor: parse_cursor(cursor),
        }),
        [CALLBACK_GROUP_TASK, action, origin, task_uid]
            if is_callback_code(action, "o", "open") =>
        {
            Some(TelegramCallback::OpenTask {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
                mode: TaskCardMode::Compact,
            })
        }
        [CALLBACK_GROUP_TASK, action, origin, task_uid, mode]
            if is_callback_code(action, "o", "open") =>
        {
            Some(TelegramCallback::OpenTask {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
                mode: parse_task_card_mode(mode)?,
            })
        }
        [CALLBACK_GROUP_TASK, action, origin, task_uid, status]
            if is_callback_code(action, "s", "status") =>
        {
            Some(TelegramCallback::UpdateTaskStatus {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
                next_status: parse_task_status(status)?,
            })
        }
        [CALLBACK_GROUP_TASK, action, origin, task_uid]
            if is_callback_code(action, "cc", "cancel_confirm") =>
        {
            Some(TelegramCallback::ConfirmTaskCancel {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_TASK, action, origin, task_uid]
            if is_callback_code(action, "cx", "cancel_execute") =>
        {
            Some(TelegramCallback::ExecuteTaskCancel {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, action, origin, task_uid]
            if is_callback_code(action, "cm", "comment") =>
        {
            Some(TelegramCallback::StartTaskCommentInput {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, action, origin, task_uid]
            if is_callback_code(action, "b", "blocker") =>
        {
            Some(TelegramCallback::StartTaskBlockerInput {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, action, origin, task_uid]
            if is_callback_code(action, "r", "reassign") =>
        {
            Some(TelegramCallback::StartTaskReassignInput {
                origin: parse_origin_code(origin)?,
                task_uid: Uuid::parse_str(task_uid).ok()?,
            })
        }
        [CALLBACK_GROUP_INPUT, action, origin, task_uid]
            if is_callback_code(action, "dh", "delivery_help") =>
        {
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
        [CALLBACK_GROUP_DRAFT, "assignee_pick", employee_id] => {
            Some(TelegramCallback::GuidedAssigneeConfirm {
                employee_id: employee_id.parse::<i64>().ok()?,
            })
        }
        [CALLBACK_GROUP_ADMIN, "menu"] => Some(TelegramCallback::AdminMenu),
        [CALLBACK_GROUP_ADMIN, "users"] => Some(TelegramCallback::AdminUsers),
        [CALLBACK_GROUP_ADMIN, "user", user_id] => Some(TelegramCallback::AdminUserDetails {
            user_id: user_id.parse::<i64>().ok()?,
        }),
        [CALLBACK_GROUP_ADMIN, "role", user_id, role_code] => {
            Some(TelegramCallback::AdminUserPrepareRoleChange {
                user_id: user_id.parse::<i64>().ok()?,
                next_role: AdminRoleOption::from_code(role_code)?,
            })
        }
        [CALLBACK_GROUP_ADMIN, "deact", user_id] => {
            Some(TelegramCallback::AdminUserPrepareDeactivate {
                user_id: user_id.parse::<i64>().ok()?,
            })
        }
        [CALLBACK_GROUP_ADMIN, "react", user_id] => {
            Some(TelegramCallback::AdminUserPrepareReactivate {
                user_id: user_id.parse::<i64>().ok()?,
            })
        }
        [CALLBACK_GROUP_ADMIN, "confirm", nonce] => Some(TelegramCallback::AdminConfirmNonce {
            nonce: (*nonce).to_owned(),
        }),
        [CALLBACK_GROUP_ADMIN, "cancel"] => Some(TelegramCallback::AdminCancelPending),
        [CALLBACK_GROUP_ADMIN, "audit"] => Some(TelegramCallback::AdminAudit),
        [CALLBACK_GROUP_ADMIN, "sec_audit"] => Some(TelegramCallback::AdminSecurityAudit),
        [CALLBACK_GROUP_ADMIN, "feat"] => Some(TelegramCallback::AdminFeatures),
        [CALLBACK_GROUP_ADMIN, "ftog", flag_key] => Some(TelegramCallback::AdminToggleFeature {
            flag_key: (*flag_key).to_owned(),
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

fn is_callback_code(value: &str, short: &str, legacy: &str) -> bool {
    value == short || value == legacy
}

fn origin_code(origin: TaskListOrigin) -> &'static str {
    match origin {
        TaskListOrigin::Assigned => "a",
        TaskListOrigin::Created => "c",
        TaskListOrigin::Team => "t",
        TaskListOrigin::Focus => "f",
        TaskListOrigin::ManagerInbox => "m",
    }
}

fn parse_origin_code(value: &str) -> Option<TaskListOrigin> {
    match value {
        "a" | "assigned" => Some(TaskListOrigin::Assigned),
        "c" | "created" => Some(TaskListOrigin::Created),
        "t" | "team" => Some(TaskListOrigin::Team),
        "f" | "focus" => Some(TaskListOrigin::Focus),
        "m" | "manager_inbox" => Some(TaskListOrigin::ManagerInbox),
        _ => None,
    }
}

fn task_card_mode_code(mode: TaskCardMode) -> &'static str {
    match mode {
        TaskCardMode::Compact => "c",
        TaskCardMode::Expanded => "e",
    }
}

fn parse_task_card_mode(value: &str) -> Option<TaskCardMode> {
    match value {
        "c" | "compact" => Some(TaskCardMode::Compact),
        "e" | "expanded" => Some(TaskCardMode::Expanded),
        _ => None,
    }
}

fn task_status_code(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Created => "n",
        TaskStatus::Sent => "s",
        TaskStatus::InProgress => "p",
        TaskStatus::Blocked => "b",
        TaskStatus::InReview => "r",
        TaskStatus::Completed => "d",
        TaskStatus::Cancelled => "x",
    }
}

fn parse_task_status(value: &str) -> Option<TaskStatus> {
    match value {
        "n" | "created" => Some(TaskStatus::Created),
        "s" | "sent" => Some(TaskStatus::Sent),
        "p" | "in_progress" => Some(TaskStatus::InProgress),
        "b" | "blocked" => Some(TaskStatus::Blocked),
        "r" | "in_review" => Some(TaskStatus::InReview),
        "d" | "completed" => Some(TaskStatus::Completed),
        "x" | "cancelled" => Some(TaskStatus::Cancelled),
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

    use super::{encode_callback, parse_callback, TELEGRAM_CALLBACK_DATA_MAX_BYTES};
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
    fn given_task_callbacks_when_encoded_then_fit_telegram_callback_limit() {
        let task_uid = Uuid::parse_str("019df794-9946-7c32-9483-f0e8c21e37ce").unwrap();
        let origins = [
            TaskListOrigin::Assigned,
            TaskListOrigin::Created,
            TaskListOrigin::Team,
            TaskListOrigin::Focus,
            TaskListOrigin::ManagerInbox,
        ];
        let statuses = [
            TaskStatus::Created,
            TaskStatus::Sent,
            TaskStatus::InProgress,
            TaskStatus::Blocked,
            TaskStatus::InReview,
            TaskStatus::Completed,
            TaskStatus::Cancelled,
        ];
        let modes = [TaskCardMode::Compact, TaskCardMode::Expanded];
        let mut callbacks = Vec::new();

        for origin in origins {
            callbacks.push(TelegramCallback::ListTasks {
                origin,
                cursor: None,
            });
            callbacks.push(TelegramCallback::ListTasks {
                origin,
                cursor: Some(task_uid.to_string()),
            });
            callbacks.push(TelegramCallback::ConfirmTaskCancel { task_uid, origin });
            callbacks.push(TelegramCallback::ExecuteTaskCancel { task_uid, origin });
            callbacks.push(TelegramCallback::StartTaskCommentInput { task_uid, origin });
            callbacks.push(TelegramCallback::StartTaskBlockerInput { task_uid, origin });
            callbacks.push(TelegramCallback::StartTaskReassignInput { task_uid, origin });
            callbacks.push(TelegramCallback::ShowDeliveryHelp { task_uid, origin });

            for mode in modes {
                callbacks.push(TelegramCallback::OpenTask {
                    task_uid,
                    origin,
                    mode,
                });
            }

            for next_status in statuses {
                callbacks.push(TelegramCallback::UpdateTaskStatus {
                    task_uid,
                    next_status,
                    origin,
                });
            }
        }

        for callback in callbacks {
            let encoded = encode_callback(&callback);
            assert!(
                encoded.len() <= TELEGRAM_CALLBACK_DATA_MAX_BYTES,
                "callback_data exceeds Telegram limit: len={} data={encoded}",
                encoded.len()
            );
            assert_eq!(parse_callback(&encoded), Some(callback));
        }
    }

    #[test]
    fn given_legacy_long_manager_inbox_callbacks_when_parse_then_backward_compatible() {
        let task_uid = Uuid::parse_str("019df794-9946-7c32-9483-f0e8c21e37ce").unwrap();

        let cases = [
            (
                format!("t:status:manager_inbox:{task_uid}:in_progress"),
                TelegramCallback::UpdateTaskStatus {
                    task_uid,
                    next_status: TaskStatus::InProgress,
                    origin: TaskListOrigin::ManagerInbox,
                },
            ),
            (
                format!("t:open:manager_inbox:{task_uid}:expanded"),
                TelegramCallback::OpenTask {
                    task_uid,
                    origin: TaskListOrigin::ManagerInbox,
                    mode: TaskCardMode::Expanded,
                },
            ),
            (
                format!("i:delivery_help:manager_inbox:{task_uid}"),
                TelegramCallback::ShowDeliveryHelp {
                    task_uid,
                    origin: TaskListOrigin::ManagerInbox,
                },
            ),
        ];

        for (encoded, expected) in cases {
            assert_eq!(parse_callback(&encoded), Some(expected));
        }
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

    #[test]
    fn given_admin_menu_callback_when_roundtrip_then_it_parses_back() {
        let callback = TelegramCallback::AdminMenu;

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_admin_role_change_callback_when_roundtrip_then_roles_preserved() {
        use crate::presentation::telegram::callbacks::AdminRoleOption;

        for role in [
            AdminRoleOption::User,
            AdminRoleOption::Manager,
            AdminRoleOption::Admin,
        ] {
            let callback = TelegramCallback::AdminUserPrepareRoleChange {
                user_id: 77,
                next_role: role,
            };

            let encoded = encode_callback(&callback);
            let parsed = parse_callback(&encoded);

            assert_eq!(parsed, Some(callback));
        }
    }

    #[test]
    fn given_admin_confirm_nonce_callback_when_roundtrip_then_nonce_preserved() {
        let callback = TelegramCallback::AdminConfirmNonce {
            nonce: "abc123".to_owned(),
        };

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_admin_toggle_feature_callback_when_roundtrip_then_key_preserved() {
        let callback = TelegramCallback::AdminToggleFeature {
            flag_key: "admin_panel".to_owned(),
        };

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_guided_assignee_confirm_when_roundtrip_then_employee_id_preserved() {
        let callback = TelegramCallback::GuidedAssigneeConfirm { employee_id: 99 };

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_admin_security_audit_callback_when_roundtrip_then_it_parses_back() {
        let callback = TelegramCallback::AdminSecurityAudit;

        let encoded = encode_callback(&callback);
        let parsed = parse_callback(&encoded);

        assert_eq!(parsed, Some(callback));
    }

    #[test]
    fn given_help_section_callbacks_when_roundtrip_then_each_section_preserved() {
        use crate::presentation::telegram::callbacks::HelpSection;

        for section in [
            HelpSection::Tasks,
            HelpSection::Voice,
            HelpSection::Notifications,
            HelpSection::Manager,
            HelpSection::Admin,
        ] {
            let callback = TelegramCallback::MenuHelpSection { section };
            let encoded = encode_callback(&callback);
            let parsed = parse_callback(&encoded);
            assert_eq!(
                parsed,
                Some(callback),
                "roundtrip failed for help section {section:?}"
            );
        }
    }

    #[test]
    fn given_help_section_callback_when_unknown_code_then_parse_returns_none() {
        // Защищаемся от мусорных кодов — устойчивость к подделанным callback'ам.
        let parsed = parse_callback("m:hsec:totally_unknown");
        assert_eq!(parsed, None);
    }
}
