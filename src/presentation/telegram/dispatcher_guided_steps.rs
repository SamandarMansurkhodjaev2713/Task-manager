use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::use_cases::assignee_resolution::AssigneeResolution;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::drafts::{CreationSession, GuidedTaskDraft, GuidedTaskStep};
use crate::presentation::telegram::ui;
use crate::shared::constants::limits::MIN_TASK_DESCRIPTION_LENGTH;

use super::dispatcher_transport::send_screen;
use super::{
    TelegramRuntime, GUIDED_FALLBACK_NAME, GUIDED_SYNTHETIC_MESSAGE_ID,
    GUIDED_TEXT_REQUIRED_MESSAGE,
};

pub(crate) async fn handle_guided_message(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    draft: GuidedTaskDraft,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    let Some(text) = incoming_message.text_payload() else {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::GuidedStep(draft.step),
            GUIDED_TEXT_REQUIRED_MESSAGE,
            ui::main_menu_keyboard(&actor),
        )
        .await;
    };

    let trimmed_text = text.trim();
    if trimmed_text.is_empty() {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::GuidedStep(draft.step),
            GUIDED_TEXT_REQUIRED_MESSAGE,
            ui::main_menu_keyboard(&actor),
        )
        .await;
    }

    match draft.step {
        GuidedTaskStep::Assignee => {
            handle_guided_assignee_step(bot, state, chat_id, trimmed_text).await
        }
        GuidedTaskStep::Description => {
            handle_guided_description_step(bot, state, chat_id, draft, trimmed_text).await
        }
        GuidedTaskStep::Deadline => {
            handle_guided_deadline_step(bot, state, chat_id, draft, trimmed_text).await
        }
        GuidedTaskStep::Confirm => {
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
                &ui::guided_confirmation_text(&draft),
                ui::guided_confirmation_keyboard(),
            )
            .await
        }
    }
}

pub(crate) async fn show_guided_confirmation(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: i64,
    mut draft: GuidedTaskDraft,
) -> Result<(), teloxide::RequestError> {
    draft.step = GuidedTaskStep::Confirm;
    state
        .creation_sessions
        .update_guided(chat_id, draft.clone())
        .await;
    send_screen(
        bot,
        state,
        ChatId(chat_id),
        ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
        &ui::guided_confirmation_text(&draft),
        ui::guided_confirmation_keyboard(),
    )
    .await
}

pub(crate) fn build_guided_message(
    chat_id: i64,
    actor: &User,
    draft: &GuidedTaskDraft,
    description: &str,
) -> IncomingMessage {
    // When the assignee was pre-resolved during the Assignee step, we do NOT
    // include the raw assignee text in the synthetic message.  The
    // `TaskAssigneeDecision::EmployeeId` path in the use case bypasses text-based
    // assignee parsing entirely, so including a raw abbreviation like "abd"
    // in the message would cause it to bleed into the task description.
    let base_text = if draft.resolved_employee_id.is_some() {
        description.to_owned()
    } else {
        match draft.assignee.as_deref() {
            Some(assignee) => format!("{assignee}, {description}"),
            None => description.to_owned(),
        }
    };
    let deadline_suffix = draft
        .deadline
        .as_ref()
        .map(|value| build_deadline_suffix(value))
        .unwrap_or_default();
    let text = format!("{base_text}{deadline_suffix}");

    IncomingMessage {
        message_id: GUIDED_SYNTHETIC_MESSAGE_ID,
        chat_id,
        sender_id: actor.telegram_id,
        sender_name: actor
            .full_name
            .clone()
            .unwrap_or_else(|| GUIDED_FALLBACK_NAME.to_owned()),
        sender_username: actor.telegram_username.clone(),
        content: MessageContent::Text { text },
        timestamp: chrono::Utc::now(),
        source_message_key_override: Some(format!(
            "telegram:guided:{chat_id}:{}",
            draft.submission_key
        )),
        is_voice_origin: false,
    }
}

async fn handle_guided_assignee_step(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    value: &str,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Guided(draft)) = state.creation_sessions.get(chat_id.0).await else {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::CreateMenu,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await;
    };

    // Resolve the assignee immediately so we surface suggestions or an
    // ambiguity screen *before* the user advances to Description.
    // This prevents the silent-failure case where an abbreviation or
    // unrecognised name is accepted and only rejected at submit time.
    match state
        .create_task_use_case
        .preview_assignee_resolution(value)
        .await
    {
        Ok(AssigneeResolution::Resolved(resolved)) => {
            // Unique high-confidence match (≥ 95%) — store the employee ID
            // and advance to Description without interrupting the user.
            // We use the resolved employee's full name as the display name
            // so the confirmation screen shows "Abdullazi Zazizov" instead
            // of the raw abbreviation (e.g. "ABD") the user typed.
            let employee_id = resolved.employee.as_ref().and_then(|e| e.id);
            let display_name = resolved
                .employee
                .as_ref()
                .map(|e| e.full_name.as_str())
                .unwrap_or(value);
            let updated = update_guided_assignee_resolved(draft, display_name, employee_id);
            state
                .creation_sessions
                .update_guided(chat_id.0, updated)
                .await;
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
                &ui::guided_description_prompt(),
                ui::guided_description_keyboard(),
            )
            .await
        }
        Ok(AssigneeResolution::ClarificationRequired(request)) => {
            // Ambiguous, partial, or not-found — show candidates inline now
            // so the user picks before proceeding to Description.
            // Preserve the draft unchanged (no assignee or resolved_id set yet).
            state
                .creation_sessions
                .update_guided(chat_id.0, draft)
                .await;
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::GuidedAssigneeOptions,
                &ui::guided_assignee_clarification_text(&request.message),
                ui::guided_assignee_suggestions_keyboard(&request.candidates),
            )
            .await
        }
        Err(_) => {
            // Resolution service error (e.g. DB unavailable): degrade
            // gracefully by storing the raw text and advancing — the fuzzy
            // matcher at submit time is the safety net.
            let updated = update_guided_assignee(draft, value);
            state
                .creation_sessions
                .update_guided(chat_id.0, updated)
                .await;
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
                &ui::guided_description_prompt(),
                ui::guided_description_keyboard(),
            )
            .await
        }
    }
}

async fn handle_guided_description_step(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    draft: GuidedTaskDraft,
    value: &str,
) -> Result<(), teloxide::RequestError> {
    if let Some(validation_message) = validate_guided_description(value) {
        return send_screen(
            bot,
            state,
            chat_id,
            ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
            validation_message,
            ui::guided_description_keyboard(),
        )
        .await;
    }

    let updated = update_guided_description(draft, value);
    state
        .creation_sessions
        .update_guided(chat_id.0, updated)
        .await;
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::GuidedStep(GuidedTaskStep::Deadline),
        &ui::guided_deadline_prompt(),
        ui::guided_deadline_keyboard(),
    )
    .await
}

async fn handle_guided_deadline_step(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    draft: GuidedTaskDraft,
    value: &str,
) -> Result<(), teloxide::RequestError> {
    let updated = update_guided_deadline(draft, value);
    show_guided_confirmation(bot, state, chat_id.0, updated).await
}

fn update_guided_assignee(mut draft: GuidedTaskDraft, value: &str) -> GuidedTaskDraft {
    draft.assignee = Some(value.to_owned());
    draft.resolved_employee_id = None; // explicit: no pre-resolution
    draft.step = GuidedTaskStep::Description;
    draft
}

/// Stores the resolved employee's display name and pre-resolved employee ID.
///
/// `display_name` is the employee's canonical full name (e.g. "Abdullazi
/// Zazizov"), NOT the raw abbreviation the user typed (e.g. "ABD").
/// This ensures that the confirmation screen (`guided_confirmation_text`)
/// shows a readable name rather than a cryptic abbreviation.
///
/// Used when `preview_assignee_resolution` returns a unique high-confidence
/// match so `submit()` can bypass the fuzzy matcher entirely.
fn update_guided_assignee_resolved(
    mut draft: GuidedTaskDraft,
    display_name: &str,
    employee_id: Option<i64>,
) -> GuidedTaskDraft {
    draft.assignee = Some(display_name.to_owned());
    draft.resolved_employee_id = employee_id;
    draft.step = GuidedTaskStep::Description;
    draft
}

fn update_guided_description(mut draft: GuidedTaskDraft, value: &str) -> GuidedTaskDraft {
    draft.description = Some(value.to_owned());
    draft.step = GuidedTaskStep::Deadline;
    draft
}

fn update_guided_deadline(mut draft: GuidedTaskDraft, value: &str) -> GuidedTaskDraft {
    draft.deadline = Some(value.to_owned());
    draft.step = GuidedTaskStep::Confirm;
    draft
}

fn build_deadline_suffix(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed
        .chars()
        .next()
        .is_some_and(|symbol| symbol.is_ascii_digit())
    {
        format!(" до {trimmed}")
    } else {
        format!(" {trimmed}")
    }
}

fn validate_guided_description(value: &str) -> Option<&'static str> {
    let trimmed = value.trim();
    if trimmed.chars().count() < MIN_TASK_DESCRIPTION_LENGTH {
        return Some(
            "Описание пока слишком короткое. Напишите чуть конкретнее: что именно нужно сделать и какой результат ждём.",
        );
    }

    if trimmed.split_whitespace().count() < 2 {
        return Some(
            "Формулировка пока выглядит слишком короткой. Нужна одна нормальная рабочая фраза, а не одно слово.",
        );
    }

    None
}

#[cfg(test)]
mod tests {
    use super::{update_guided_assignee_resolved, validate_guided_description};
    use crate::presentation::telegram::drafts::{GuidedTaskDraft, GuidedTaskStep};

    #[test]
    fn given_too_short_description_when_validating_then_returns_hint() {
        let validation = validate_guided_description("сделать");

        assert!(validation.is_some());
    }

    /// When an auto-resolved high-confidence match is stored, the draft must
    /// carry the employee's *full name*, not the raw abbreviation the user
    /// typed.  This ensures the confirmation screen renders "Abdullazi Zazizov"
    /// rather than "ABD".
    #[test]
    fn given_prefix_match_when_resolved_then_draft_stores_full_name_not_abbreviation() {
        let draft = GuidedTaskDraft::new();

        let updated = update_guided_assignee_resolved(draft, "Abdullazi Zazizov", Some(42));

        assert_eq!(
            updated.assignee.as_deref(),
            Some("Abdullazi Zazizov"),
            "draft must store the resolved full name, not the raw typed abbreviation"
        );
        assert_eq!(
            updated.resolved_employee_id,
            Some(42),
            "pre-resolved employee ID must be preserved"
        );
        assert_eq!(
            updated.step,
            GuidedTaskStep::Description,
            "step must advance to Description"
        );
    }

    /// When the resolution service fails and we fall back to the raw text path,
    /// the draft stores the raw query (not a full name) — this is the safe
    /// degradation path; the fuzzy matcher handles it at submit time.
    #[test]
    fn given_fallback_path_when_raw_text_stored_then_no_resolved_id() {
        use super::update_guided_assignee;

        let draft = GuidedTaskDraft::new();

        let updated = update_guided_assignee(draft, "ABD");

        assert_eq!(
            updated.assignee.as_deref(),
            Some("ABD"),
            "fallback path stores the raw typed text"
        );
        assert_eq!(
            updated.resolved_employee_id, None,
            "fallback path must not set a resolved employee ID"
        );
        assert_eq!(updated.step, GuidedTaskStep::Description);
    }

    #[test]
    fn given_workable_description_when_validating_then_accepts_it() {
        let validation = validate_guided_description("подготовить чек-лист релиза");

        assert!(validation.is_none());
    }
}
