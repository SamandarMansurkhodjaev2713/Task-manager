use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::dto::task_views::TaskCreationOutcome;
use crate::application::use_cases::create_task_from_message::TaskAssigneeDecision;
use crate::application::use_cases::reassign_task::{ReassignAssigneeDecision, ReassignTaskOutcome};
use crate::domain::user::User;
use crate::presentation::telegram::assignee_selections::PendingAssigneeSelection;
use crate::presentation::telegram::callbacks::TaskCardMode;
use crate::presentation::telegram::ui;

use super::dispatcher_creation_outcomes::{keyboard_for_outcome, outcome_descriptor};
use super::dispatcher_guided::SessionCompletion;
use super::dispatcher_handlers::{show_task_details_with_notice, TaskScreenContext};
use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

pub(crate) async fn choose_clarified_assignee(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
    employee_id: i64,
) -> Result<(), teloxide::RequestError> {
    let Some(selection) = state.assignee_selections.get(chat_id.0).await else {
        return send_screen(
            bot,
            state,
            chat_id,
            crate::presentation::telegram::active_screens::ScreenDescriptor::CreateMenu,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    match selection {
        PendingAssigneeSelection::Create(pending) => {
            if !pending.candidate_employee_ids.contains(&employee_id) {
                state.assignee_selections.clear(chat_id.0).await;
                return send_screen(
                    bot,
                    state,
                    chat_id,
                    crate::presentation::telegram::active_screens::ScreenDescriptor::CreateMenu,
                    &ui::create_menu_text(),
                    ui::create_menu_keyboard(),
                )
                .await;
            }

            match state
                .create_task_use_case
                .execute_with_assignee_decision(
                    pending.message,
                    TaskAssigneeDecision::EmployeeId(employee_id),
                )
                .await
            {
                Ok(outcome) => {
                    state.assignee_selections.clear(chat_id.0).await;
                    state.creation_sessions.clear(chat_id.0).await;
                    let keyboard = keyboard_for_outcome(&outcome, SessionCompletion::Clear);
                    send_screen(
                        bot,
                        state,
                        chat_id,
                        outcome_descriptor(&outcome),
                        &ui::task_creation_text(&outcome),
                        keyboard,
                    )
                    .await
                }
                Err(error) => send_error(bot, state, chat_id.0, error).await,
            }
        }
        PendingAssigneeSelection::Reassign(pending) => {
            if !pending.candidate_employee_ids.contains(&employee_id) {
                state.assignee_selections.clear(chat_id.0).await;
                return show_task_details_with_notice(
                    bot,
                    state,
                    actor,
                    TaskScreenContext {
                        chat_id,
                        task_uid: pending.task_uid,
                        origin: pending.origin,
                        mode: TaskCardMode::Compact,
                    },
                    Some("Список кандидатов устарел. Задача открыта заново — попробуйте назначить ещё раз."),
                )
                .await;
            }

            match state
                .reassign_task_use_case
                .execute_with_decision(
                    actor,
                    pending.task_uid,
                    &pending.original_query,
                    ReassignAssigneeDecision::EmployeeId(employee_id),
                )
                .await
            {
                Ok(ReassignTaskOutcome::Reassigned(summary)) => {
                    state.assignee_selections.clear(chat_id.0).await;
                    show_task_details_with_notice(
                        bot,
                        state,
                        actor,
                        TaskScreenContext {
                            chat_id,
                            task_uid: pending.task_uid,
                            origin: pending.origin,
                            mode: TaskCardMode::Compact,
                        },
                        Some(&summary.message),
                    )
                    .await
                }
                Ok(ReassignTaskOutcome::ClarificationRequired(request)) => {
                    state
                        .assignee_selections
                        .set_reassign(
                            chat_id.0,
                            pending.task_uid,
                            pending.origin,
                            pending.original_query,
                            clarification_candidate_ids(&request),
                        )
                        .await;

                    send_screen(
                        bot,
                        state,
                        chat_id,
                        crate::presentation::telegram::active_screens::ScreenDescriptor::TaskInteractionPrompt {
                            task_uid: pending.task_uid,
                            origin: pending.origin,
                            kind: crate::presentation::telegram::interactions::TaskInteractionKind::Reassign,
                        },
                        &ui::task_creation_text(&TaskCreationOutcome::ClarificationRequired(
                            request.clone(),
                        )),
                        ui::clarification_keyboard(&request),
                    )
                    .await
                }
                Err(error) => send_error(bot, state, chat_id.0, error).await,
            }
        }
    }
}

pub(crate) async fn create_without_assignee_after_clarification(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    let Some(selection) = state.assignee_selections.get(chat_id.0).await else {
        return send_screen(
            bot,
            state,
            chat_id,
            crate::presentation::telegram::active_screens::ScreenDescriptor::CreateMenu,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    let PendingAssigneeSelection::Create(pending) = selection else {
        return send_screen(
            bot,
            state,
            chat_id,
            crate::presentation::telegram::active_screens::ScreenDescriptor::CreateMenu,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    if !pending.allow_unassigned {
        state.assignee_selections.clear(chat_id.0).await;
        return send_screen(
            bot,
            state,
            chat_id,
            crate::presentation::telegram::active_screens::ScreenDescriptor::CreateMenu,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    }

    match state
        .create_task_use_case
        .execute_with_assignee_decision(pending.message, TaskAssigneeDecision::CreateUnassigned)
        .await
    {
        Ok(outcome) => {
            state.assignee_selections.clear(chat_id.0).await;
            state.creation_sessions.clear(chat_id.0).await;
            let keyboard = keyboard_for_outcome(&outcome, SessionCompletion::Clear);
            send_screen(
                bot,
                state,
                chat_id,
                outcome_descriptor(&outcome),
                &ui::task_creation_text(&outcome),
                keyboard,
            )
            .await
        }
        Err(error) => send_error(bot, state, chat_id.0, error).await,
    }
}

/// Extracts the list of concrete employee IDs from a clarification request.
///
/// This is the single canonical copy; `dispatcher_guided` and `dispatcher_voice`
/// import it from here rather than duplicating it.
pub(crate) fn clarification_candidate_ids(
    request: &crate::application::dto::task_views::ClarificationRequest,
) -> Vec<i64> {
    request
        .candidates
        .iter()
        .filter_map(|candidate| candidate.employee_id)
        .collect()
}
