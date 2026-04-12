#[derive(Debug, Clone)]
pub enum BotCommand {
    Start,
    Menu,
    Help,
    NewTask { payload: Option<String> },
    MyTasks { cursor: Option<String> },
    CreatedTasks { cursor: Option<String> },
    TeamTasks { cursor: Option<String> },
    Status { task_uid: String },
    CancelTask { task_uid: String },
    Stats,
    TeamStats,
    Settings,
    AdminSyncEmployees,
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
        _ => None,
    }
}
