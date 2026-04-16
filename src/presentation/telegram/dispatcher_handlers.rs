use teloxide::prelude::Requester;
use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::use_cases::collect_stats::StatsScope;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::presentation::telegram::callbacks::TelegramCallback;
use crate::presentation::telegram::commands::BotCommand;

use super::dispatcher_guided::{
    create_task_and_present, edit_guided_field, skip_guided_assignee, skip_guided_deadline,
    start_guided_create, start_quick_create, submit_guided_draft, SessionCompletion,
};
use super::dispatcher_interactions::{
    start_task_blocker_input, start_task_comment_input, start_task_reassign_input,
};
use super::dispatcher_navigation::{
    show_create_menu, show_help, show_main_menu, show_main_menu_fresh, show_settings, show_stats,
    show_task_list, sync_employees,
};
use super::dispatcher_task_views::{
    confirm_task_cancel, execute_cancel_from_command, show_delivery_help, show_task_details,
    show_task_from_command, update_task_status,
};
use super::dispatcher_transport::send_error;
use super::dispatcher_voice::{
    cancel_voice_create, return_to_voice_confirmation, start_voice_transcript_edit,
    submit_voice_draft,
};
use super::TelegramRuntime;
use super::RATE_LIMIT_MESSAGE;

pub(crate) use super::dispatcher_task_views::{show_task_details_with_notice, TaskScreenContext};

pub(crate) async fn register_actor(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: &IncomingMessage,
) -> Result<Option<User>, teloxide::RequestError> {
    match state.register_user_use_case.execute(incoming_message).await {
        Ok(actor) => Ok(Some(actor)),
        Err(error) => {
            send_error(bot, incoming_message.chat_id, error).await?;
            Ok(None)
        }
    }
}

pub(crate) async fn check_rate_limit(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<bool, teloxide::RequestError> {
    let actor_key = u64::try_from(actor.telegram_id).unwrap_or_default();
    if state.rate_limiter.check(actor_key) {
        return Ok(true);
    }

    bot.send_message(chat_id, RATE_LIMIT_MESSAGE).await?;
    Ok(false)
}

pub(crate) async fn handle_command(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    command: BotCommand,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    state.task_interactions.clear(chat_id.0).await;

    match command {
        BotCommand::Start | BotCommand::Menu => {
            show_main_menu_fresh(bot, state, &actor, chat_id).await
        }
        BotCommand::Help => show_help(bot, state, chat_id, &actor).await,
        BotCommand::NewTask { payload } => {
            handle_new_task_command(bot, state, incoming_message, chat_id, payload).await
        }
        BotCommand::MyTasks { cursor } => {
            show_task_list(
                bot,
                state,
                &actor,
                chat_id,
                crate::presentation::telegram::callbacks::TaskListOrigin::Assigned,
                cursor,
            )
            .await
        }
        BotCommand::CreatedTasks { cursor } => {
            show_task_list(
                bot,
                state,
                &actor,
                chat_id,
                crate::presentation::telegram::callbacks::TaskListOrigin::Created,
                cursor,
            )
            .await
        }
        BotCommand::TeamTasks { cursor } => {
            show_task_list(
                bot,
                state,
                &actor,
                chat_id,
                crate::presentation::telegram::callbacks::TaskListOrigin::Team,
                cursor,
            )
            .await
        }
        BotCommand::Status { task_uid } => {
            show_task_from_command(bot, state, &actor, chat_id, &task_uid).await
        }
        BotCommand::CancelTask { task_uid } => {
            execute_cancel_from_command(bot, state, &actor, chat_id, &task_uid).await
        }
        BotCommand::Stats => show_stats(bot, state, &actor, chat_id, StatsScope::Personal).await,
        BotCommand::TeamStats => show_stats(bot, state, &actor, chat_id, StatsScope::Team).await,
        BotCommand::Settings => show_settings(bot, state, chat_id, &actor).await,
        BotCommand::AdminSyncEmployees => sync_employees(bot, state, chat_id, &actor).await,
    }
}

pub(crate) async fn handle_callback_action(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    callback: TelegramCallback,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    state.task_interactions.clear(chat_id.0).await;

    match callback {
        TelegramCallback::MenuHome => show_main_menu(bot, state, &actor, chat_id).await,
        TelegramCallback::MenuHelp => show_help(bot, state, chat_id, &actor).await,
        TelegramCallback::MenuSettings => show_settings(bot, state, chat_id, &actor).await,
        TelegramCallback::MenuStats => {
            show_stats(bot, state, &actor, chat_id, StatsScope::Personal).await
        }
        TelegramCallback::MenuTeamStats => {
            show_stats(bot, state, &actor, chat_id, StatsScope::Team).await
        }
        TelegramCallback::MenuCreate => show_create_menu(bot, state, chat_id).await,
        TelegramCallback::MenuSyncEmployees => sync_employees(bot, state, chat_id, &actor).await,
        TelegramCallback::ListTasks { origin, cursor } => {
            show_task_list(bot, state, &actor, chat_id, origin, cursor).await
        }
        TelegramCallback::OpenTask {
            task_uid,
            origin,
            mode,
        } => show_task_details(bot, state, &actor, chat_id, task_uid, origin, mode).await,
        TelegramCallback::UpdateTaskStatus {
            task_uid,
            next_status,
            origin,
        } => update_task_status(bot, state, &actor, chat_id, task_uid, next_status, origin).await,
        TelegramCallback::ConfirmTaskCancel { task_uid, origin } => {
            confirm_task_cancel(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::ExecuteTaskCancel { task_uid, origin } => {
            update_task_status(
                bot,
                state,
                &actor,
                chat_id,
                task_uid,
                crate::domain::task::TaskStatus::Cancelled,
                origin,
            )
            .await
        }
        TelegramCallback::StartTaskCommentInput { task_uid, origin } => {
            start_task_comment_input(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::StartTaskBlockerInput { task_uid, origin } => {
            start_task_blocker_input(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::StartTaskReassignInput { task_uid, origin } => {
            start_task_reassign_input(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::ShowDeliveryHelp { task_uid, origin } => {
            show_delivery_help(bot, state, &actor, chat_id, task_uid, origin).await
        }
        TelegramCallback::StartQuickCreate => start_quick_create(bot, state, chat_id).await,
        TelegramCallback::StartGuidedCreate => start_guided_create(bot, state, chat_id).await,
        TelegramCallback::VoiceCreateConfirm => {
            submit_voice_draft(bot, state, &actor, chat_id).await
        }
        TelegramCallback::VoiceCreateEdit => start_voice_transcript_edit(bot, state, chat_id).await,
        TelegramCallback::VoiceCreateBack => {
            return_to_voice_confirmation(bot, state, chat_id).await
        }
        TelegramCallback::VoiceCreateCancel => cancel_voice_create(bot, state, chat_id).await,
        TelegramCallback::DraftSkipAssignee => skip_guided_assignee(bot, state, chat_id).await,
        TelegramCallback::DraftSkipDeadline => skip_guided_deadline(bot, state, chat_id).await,
        TelegramCallback::DraftSubmit => submit_guided_draft(bot, state, &actor, chat_id).await,
        TelegramCallback::DraftEdit { field } => {
            edit_guided_field(bot, state, chat_id, field).await
        }
    }
}

async fn handle_new_task_command(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    chat_id: ChatId,
    payload: Option<String>,
) -> Result<(), teloxide::RequestError> {
    match payload {
        Some(payload) => {
            let synthetic_message = IncomingMessage {
                content: MessageContent::Text { text: payload },
                ..incoming_message
            };
            create_task_and_present(
                bot,
                state,
                chat_id,
                synthetic_message,
                SessionCompletion::Clear,
            )
            .await
        }
        None => show_create_menu(bot, state, chat_id).await,
    }
}
