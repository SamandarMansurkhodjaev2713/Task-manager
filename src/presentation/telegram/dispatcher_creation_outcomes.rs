use crate::application::dto::task_views::TaskCreationOutcome;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::ui;
use teloxide::types::InlineKeyboardMarkup;

use super::dispatcher_guided::SessionCompletion;

pub(crate) fn keyboard_for_outcome(
    outcome: &TaskCreationOutcome,
    session_completion: SessionCompletion,
) -> InlineKeyboardMarkup {
    match session_completion {
        SessionCompletion::KeepOnClarification
            if matches!(outcome, TaskCreationOutcome::ClarificationRequired(_)) =>
        {
            ui::create_menu_keyboard()
        }
        _ => ui::outcome_keyboard(outcome),
    }
}

pub(crate) fn outcome_descriptor(outcome: &TaskCreationOutcome) -> ScreenDescriptor {
    match outcome {
        TaskCreationOutcome::Created(summary) | TaskCreationOutcome::DuplicateFound(summary) => {
            ScreenDescriptor::TaskCreationResult {
                task_uid: Some(summary.task_uid),
            }
        }
        TaskCreationOutcome::ClarificationRequired(_) => {
            ScreenDescriptor::TaskCreationResult { task_uid: None }
        }
    }
}

pub(crate) fn should_clear_session(
    outcome: &TaskCreationOutcome,
    completion: SessionCompletion,
) -> bool {
    match completion {
        SessionCompletion::Clear => true,
        SessionCompletion::KeepOnClarification => {
            matches!(
                outcome,
                TaskCreationOutcome::Created(_) | TaskCreationOutcome::DuplicateFound(_)
            )
        }
    }
}
