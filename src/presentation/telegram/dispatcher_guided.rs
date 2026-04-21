//! Guided and quick-capture task creation flow coordinator.
//!
//! [`GuidedCreateCoordinator`] owns the full lifecycle of both creation modes:
//!
//! **Quick-capture** — the user sends a free-form message or voice note;
//! the use case executes immediately with no additional input.
//!
//! **Guided (step-by-step)** — the user walks through Assignee → Description
//! → Deadline → Confirm steps before submitting.
//!
//! Public entry points used by `dispatcher_handlers.rs`:
//!
//! ```text
//! start_quick()      — enter quick-capture mode
//! start_guided()     — enter step-by-step mode
//! skip_assignee()    — advance past the Assignee step without a value
//! skip_deadline()    — advance past the Deadline step without a value
//! edit_field(field)  — return to a specific step for editing
//! submit(actor)      — submit the complete guided draft
//! create_and_present — execute create_task for quick-capture / /task command
//! ```
//!
//! Free-function wrappers (`handle_creation_session_message`, `create_task_and_present`)
//! are retained for callers that do not yet use the coordinator directly.

use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::dto::task_views::TaskCreationOutcome;
use crate::application::use_cases::create_task_from_message::TaskAssigneeDecision;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::callbacks::DraftEditField;
use crate::presentation::telegram::drafts::{CreationSession, GuidedTaskStep};
use crate::presentation::telegram::ui;

use super::dispatcher_assignee_clarifications::clarification_candidate_ids;
use super::dispatcher_creation_outcomes::{
    keyboard_for_outcome, outcome_descriptor, should_clear_session,
};
use super::dispatcher_guided_steps::{
    build_guided_message, handle_guided_message, show_guided_confirmation,
};
use super::dispatcher_transport::{send_error, send_screen};
use super::dispatcher_voice::VoiceCreateCoordinator;
use super::{TelegramRuntime, GUIDED_DESCRIPTION_REQUIRED_MESSAGE};

// ─── Coordinator ──────────────────────────────────────────────────────────────

/// Stateless coordinator for the guided and quick-capture task-creation flows.
///
/// Constructed per-request; all mutable state lives in `state.creation_sessions`
/// and `state.assignee_selections`.
pub(crate) struct GuidedCreateCoordinator<'a> {
    bot: &'a Bot,
    state: &'a TelegramRuntime,
}

impl<'a> GuidedCreateCoordinator<'a> {
    pub fn new(bot: &'a Bot, state: &'a TelegramRuntime) -> Self {
        Self { bot, state }
    }

    /// Enters quick-capture mode: clears previous selections, sets the session,
    /// and shows the minimal quick-capture keyboard.
    pub async fn start_quick(&self, chat_id: ChatId) -> Result<(), teloxide::RequestError> {
        self.state.assignee_selections.clear(chat_id.0).await;
        self.state
            .creation_sessions
            .set_quick_capture(chat_id.0)
            .await;
        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::QuickCreate,
            &ui::quick_create_prompt(),
            // quick_capture_keyboard instead of create_menu_keyboard: the user is already
            // in capture mode so mode-switch buttons would cause a ScreenDescriptor mismatch
            // stale-callback response when clicked.
            ui::quick_capture_keyboard(),
        )
        .await
    }

    /// Enters guided (step-by-step) mode: clears previous selections and shows
    /// the first step — Assignee.
    pub async fn start_guided(&self, chat_id: ChatId) -> Result<(), teloxide::RequestError> {
        self.state.assignee_selections.clear(chat_id.0).await;
        self.state.creation_sessions.set_guided(chat_id.0).await;
        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::GuidedStep(GuidedTaskStep::Assignee),
            &ui::guided_assignee_prompt(),
            ui::guided_assignee_keyboard(),
        )
        .await
    }

    /// Confirms a specific employee during the guided Assignee step.
    ///
    /// Called when the user clicks a candidate button on the
    /// `GuidedAssigneeOptions` screen.  Stores the employee ID in the draft
    /// and advances to Description without re-running any fuzzy matching.
    pub async fn confirm_assignee(
        &self,
        chat_id: ChatId,
        employee_id: i64,
    ) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Guided(mut draft)) =
            self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        draft.resolved_employee_id = Some(employee_id);
        draft.step = GuidedTaskStep::Description;
        self.state
            .creation_sessions
            .update_guided(chat_id.0, draft)
            .await;

        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
            &ui::guided_description_prompt(),
            ui::create_menu_keyboard(),
        )
        .await
    }

    /// Routes an incoming message to the correct handler for the active session type.
    ///
    /// - `QuickCapture` with a voice message → delegates to [`VoiceCreateCoordinator`].
    /// - `QuickCapture` with text → executes the task creation immediately.
    /// - `Guided` → passes the message to the step handler.
    /// - `Voice` → delegates to [`VoiceCreateCoordinator`].
    pub async fn handle_session_message(
        &self,
        incoming_message: IncomingMessage,
        actor: User,
        session: CreationSession,
    ) -> Result<(), teloxide::RequestError> {
        match session {
            CreationSession::QuickCapture => {
                if matches!(&incoming_message.content, MessageContent::Voice { .. }) {
                    return VoiceCreateCoordinator::new(self.bot, self.state)
                        .start(&actor, incoming_message)
                        .await;
                }
                self.create_and_present(
                    ChatId(incoming_message.chat_id),
                    incoming_message,
                    SessionCompletion::KeepOnClarification,
                )
                .await
            }
            CreationSession::Guided(draft) => {
                handle_guided_message(self.bot, self.state, incoming_message, actor, draft).await
            }
            CreationSession::Voice(draft) => {
                VoiceCreateCoordinator::new(self.bot, self.state)
                    .handle_message(incoming_message, &actor, draft)
                    .await
            }
        }
    }

    /// Skips the Assignee step and advances to Description.
    pub async fn skip_assignee(&self, chat_id: ChatId) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Guided(mut draft)) =
            self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        draft.assignee = None;
        draft.step = GuidedTaskStep::Description;
        self.state
            .creation_sessions
            .update_guided(chat_id.0, draft)
            .await;
        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
            &ui::guided_description_prompt(),
            ui::create_menu_keyboard(),
        )
        .await
    }

    /// Skips the Deadline step and advances to Confirm.
    pub async fn skip_deadline(&self, chat_id: ChatId) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Guided(mut draft)) =
            self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        draft.deadline = None;
        draft.step = GuidedTaskStep::Confirm;
        show_guided_confirmation(self.bot, self.state, chat_id.0, draft).await
    }

    /// Returns to a specific draft field for editing, then shows that step's screen.
    pub async fn edit_field(
        &self,
        chat_id: ChatId,
        field: DraftEditField,
    ) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Guided(mut draft)) =
            self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        draft.edit_field(field);
        self.state
            .creation_sessions
            .update_guided(chat_id.0, draft.clone())
            .await;

        match draft.step {
            GuidedTaskStep::Assignee => {
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::GuidedStep(GuidedTaskStep::Assignee),
                    &ui::guided_assignee_prompt(),
                    ui::guided_assignee_keyboard(),
                )
                .await
            }
            GuidedTaskStep::Description => {
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::GuidedStep(GuidedTaskStep::Description),
                    &ui::guided_description_prompt(),
                    ui::create_menu_keyboard(),
                )
                .await
            }
            GuidedTaskStep::Deadline => {
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::GuidedStep(GuidedTaskStep::Deadline),
                    &ui::guided_deadline_prompt(),
                    ui::guided_deadline_keyboard(),
                )
                .await
            }
            GuidedTaskStep::Confirm => {
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
                    &ui::guided_confirmation_text(&draft),
                    ui::guided_confirmation_keyboard(),
                )
                .await
            }
        }
    }

    /// Submits the guided draft to the `create_task` use case.
    ///
    /// - If the draft lacks a description, stays on the Confirm screen and shows an error.
    /// - On `ClarificationRequired`, stores candidates and shows the employee-selection keyboard.
    /// - On success, clears the session and shows the creation result.
    pub async fn submit(
        &self,
        actor: &User,
        chat_id: ChatId,
    ) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Guided(draft)) =
            self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        let Some(description) = draft
            .description
            .as_ref()
            .filter(|value| !value.trim().is_empty())
        else {
            return send_screen(
                self.bot,
                self.state,
                chat_id,
                ScreenDescriptor::GuidedStep(GuidedTaskStep::Confirm),
                GUIDED_DESCRIPTION_REQUIRED_MESSAGE,
                ui::guided_confirmation_keyboard(),
            )
            .await;
        };

        let synthetic_message = build_guided_message(chat_id.0, actor, &draft, description);
        // When the assignee was pre-resolved during the Assignee step, skip
        // the fuzzy matcher entirely by passing the confirmed employee ID.
        // This prevents re-parsing the raw input text (which may be an
        // abbreviation that the regex/Levenshtein path cannot handle), and
        // guarantees no silent wrong assignment.
        let creation_result = match draft.resolved_employee_id {
            Some(employee_id) => {
                self.state
                    .create_task_use_case
                    .execute_with_assignee_decision(
                        synthetic_message.clone(),
                        TaskAssigneeDecision::EmployeeId(employee_id),
                    )
                    .await
            }
            None => {
                self.state
                    .create_task_use_case
                    .execute(synthetic_message.clone())
                    .await
            }
        };
        match creation_result {
            Ok(outcome @ TaskCreationOutcome::Created(_))
            | Ok(outcome @ TaskCreationOutcome::DuplicateFound(_)) => {
                self.state.creation_sessions.clear(chat_id.0).await;
                self.state.assignee_selections.clear(chat_id.0).await;
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    outcome_descriptor(&outcome),
                    &ui::task_creation_text(&outcome),
                    ui::outcome_keyboard(&outcome),
                )
                .await
            }
            Ok(ref outcome @ TaskCreationOutcome::ClarificationRequired(ref request)) => {
                self.state
                    .assignee_selections
                    .set_create(
                        chat_id.0,
                        synthetic_message,
                        clarification_candidate_ids(request),
                        request.allow_unassigned,
                    )
                    .await;
                // Show the clarification keyboard so the user can pick the correct employee.
                // guided_confirmation_keyboard would hide the employee-selection buttons entirely.
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    outcome_descriptor(outcome),
                    &ui::task_creation_text(outcome),
                    ui::clarification_keyboard(request),
                )
                .await
            }
            Err(error) => send_error(self.bot, self.state, chat_id.0, error).await,
        }
    }

    /// Executes the `create_task` use case for a fully-formed `IncomingMessage`
    /// and shows the appropriate result screen.
    ///
    /// Used for the quick-capture session and the `/task <payload>` command.
    pub async fn create_and_present(
        &self,
        chat_id: ChatId,
        incoming_message: IncomingMessage,
        session_completion: SessionCompletion,
    ) -> Result<(), teloxide::RequestError> {
        match self
            .state
            .create_task_use_case
            .execute(incoming_message.clone())
            .await
        {
            Ok(outcome) => {
                if let TaskCreationOutcome::ClarificationRequired(request) = &outcome {
                    self.state
                        .assignee_selections
                        .set_create(
                            chat_id.0,
                            incoming_message.clone(),
                            clarification_candidate_ids(request),
                            request.allow_unassigned,
                        )
                        .await;
                } else {
                    self.state.assignee_selections.clear(chat_id.0).await;
                }
                if should_clear_session(&outcome, session_completion) {
                    self.state.creation_sessions.clear(chat_id.0).await;
                }
                let keyboard = keyboard_for_outcome(&outcome, session_completion);
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    outcome_descriptor(&outcome),
                    &ui::task_creation_text(&outcome),
                    keyboard,
                )
                .await
            }
            Err(error) => send_error(self.bot, self.state, chat_id.0, error).await,
        }
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    async fn fallback_to_create_menu(&self, chat_id: ChatId) -> Result<(), teloxide::RequestError> {
        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::CreateMenu,
            &ui::create_menu_text(),
            ui::create_menu_keyboard(),
        )
        .await
    }
}

// ─── Public session-completion marker ─────────────────────────────────────────

/// Controls whether the creation session is cleared after `create_and_present`.
///
/// `KeepOnClarification` prevents the session from being dropped when the
/// user needs to resolve an ambiguous assignee — the session must stay alive
/// so the subsequent clarification callback can re-submit with the chosen employee.
#[derive(Clone, Copy)]
pub(crate) enum SessionCompletion {
    Clear,
    KeepOnClarification,
}

// ─── Free-function wrappers (backward-compat shims) ───────────────────────────
//
// These exist so `dispatcher.rs` and `dispatcher_handlers.rs` can call the
// coordinator without an explicit struct construction everywhere.  They will be
// inlined or removed in a future cleanup pass.

pub(crate) async fn handle_creation_session_message(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    actor: User,
    session: CreationSession,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .handle_session_message(incoming_message, actor, session)
        .await
}

pub(crate) async fn start_quick_create(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .start_quick(chat_id)
        .await
}

pub(crate) async fn start_guided_create(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .start_guided(chat_id)
        .await
}

pub(crate) async fn skip_guided_assignee(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .skip_assignee(chat_id)
        .await
}

pub(crate) async fn skip_guided_deadline(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .skip_deadline(chat_id)
        .await
}

pub(crate) async fn edit_guided_field(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    field: DraftEditField,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .edit_field(chat_id, field)
        .await
}

pub(crate) async fn submit_guided_draft(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .submit(actor, chat_id)
        .await
}

pub(crate) async fn create_task_and_present(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    incoming_message: IncomingMessage,
    session_completion: SessionCompletion,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .create_and_present(chat_id, incoming_message, session_completion)
        .await
}

pub(crate) async fn confirm_guided_assignee(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    employee_id: i64,
) -> Result<(), teloxide::RequestError> {
    GuidedCreateCoordinator::new(bot, state)
        .confirm_assignee(chat_id, employee_id)
        .await
}
