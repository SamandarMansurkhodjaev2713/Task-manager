#[derive(Debug, Clone)]
pub enum BotCommand {
    Start,
    Menu,
    Help,
    NewTask {
        payload: Option<String>,
    },
    MyTasks {
        cursor: Option<String>,
    },
    CreatedTasks {
        cursor: Option<String>,
    },
    TeamTasks {
        cursor: Option<String>,
    },
    Status {
        task_uid: String,
    },
    CancelTask {
        task_uid: String,
    },
    Stats,
    TeamStats,
    Settings,
    AdminSyncEmployees,
    /// Opens the in-Telegram admin panel.  Access is gated by
    /// [`RoleAuthorizationPolicy::ensure_can_access_admin_panel`] — non-admins
    /// receive a polite rejection, not silence, so misconfigured accounts
    /// are easy to diagnose.
    Admin,
    /// `/find <query>` — substring search across the user's tasks
    /// (Phase 10 skeleton: returns a placeholder response; full
    /// implementation lands in a follow-up phase).  The payload is kept
    /// optional so the parser can distinguish "no query" (which we'll
    /// answer with a usage hint) from "empty query" (which trims to None).
    Find {
        query: Option<String>,
    },
}

pub fn parse_command(text: &str) -> Option<BotCommand> {
    let normalized_text = text.trim();
    let mut parts = normalized_text.splitn(2, char::is_whitespace);
    let command = parts.next()?.split('@').next()?;
    let payload = parts
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match command {
        "/start" => Some(BotCommand::Start),
        "/menu" => Some(BotCommand::Menu),
        "/help" => Some(BotCommand::Help),
        "/new_task" => Some(BotCommand::NewTask {
            payload: payload.map(ToOwned::to_owned),
        }),
        "/my_tasks" => Some(BotCommand::MyTasks {
            cursor: payload.map(ToOwned::to_owned),
        }),
        "/created_tasks" => Some(BotCommand::CreatedTasks {
            cursor: payload.map(ToOwned::to_owned),
        }),
        "/team_tasks" => Some(BotCommand::TeamTasks {
            cursor: payload.map(ToOwned::to_owned),
        }),
        "/status" => payload.map(|task_uid| BotCommand::Status {
            task_uid: task_uid.to_owned(),
        }),
        "/cancel_task" => payload.map(|task_uid| BotCommand::CancelTask {
            task_uid: task_uid.to_owned(),
        }),
        "/stats" => Some(BotCommand::Stats),
        "/team_stats" => Some(BotCommand::TeamStats),
        "/settings" => Some(BotCommand::Settings),
        "/admin_sync_employees" => Some(BotCommand::AdminSyncEmployees),
        "/admin" => Some(BotCommand::Admin),
        "/find" => Some(BotCommand::Find {
            query: payload.map(ToOwned::to_owned),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_command, BotCommand};

    #[test]
    fn given_find_with_payload_when_parsed_then_yields_query() {
        match parse_command("/find release notes") {
            Some(BotCommand::Find { query: Some(q) }) => {
                assert_eq!(q, "release notes");
            }
            other => panic!("expected Find with payload, got {other:?}"),
        }
    }

    #[test]
    fn given_find_without_payload_when_parsed_then_yields_no_query() {
        match parse_command("/find") {
            Some(BotCommand::Find { query: None }) => (),
            other => panic!("expected Find without payload, got {other:?}"),
        }
    }

    #[test]
    fn given_find_with_bot_mention_when_parsed_then_drops_mention() {
        match parse_command("/find@my_bot important") {
            Some(BotCommand::Find { query: Some(q) }) => {
                assert_eq!(q, "important");
            }
            other => panic!("expected Find with payload, got {other:?}"),
        }
    }
}
