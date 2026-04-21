//! Voice-creation flow coordinator.
//!
//! [`VoiceCreateCoordinator`] owns the full lifecycle of a voice-based task
//! creation session:
//!
//! ```text
//! start()           — transcribe audio, store VoiceTaskDraft, show confirmation
//! handle_message()  — route incoming text/voice during the active session
//! start_edit()      — enter transcript-edit sub-state
//! return_to_confirmation() — back to Confirm step after editing
//! cancel()          — clear session, go to CreateMenu
//! submit()          — execute create_task use case, clear session on success
//! ```
//!
//! All Telegram I/O is handled internally; callers only supply the actor and
//! a `ChatId`/`IncomingMessage` as appropriate.

use teloxide::types::{ChatId, InlineKeyboardMarkup};
use teloxide::Bot;

use crate::application::dto::task_views::TaskCreationOutcome;
use crate::domain::errors::AppError;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::domain::voice_transcript::NormalizedTranscript;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::drafts::{CreationSession, VoiceTaskDraft, VoiceTaskStep};
use crate::presentation::telegram::ui;

use super::dispatcher_assignee_clarifications::clarification_candidate_ids;
use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

// ─── Constants ────────────────────────────────────────────────────────────────

const VOICE_SYNTHETIC_MESSAGE_ID: i32 = -1;
const VOICE_TEXT_REQUIRED_MESSAGE: &str =
    "Нужен один короткий текст сообщением. Я заменю им расшифровку и снова покажу подтверждение.";
const VOICE_TRANSCRIPT_UPDATED_NOTICE: &str =
    "✏️ Текст обновлён. Проверьте финальную версию перед созданием.";
const VOICE_CONFIRMATION_HINT: &str =
    "Если что-то понято неточно, нажмите «Исправить текст» или отправьте новое голосовое.";
const VOICE_PROCESSING_MESSAGE: &str = "⏳ Распознаю голосовое...";
const VOICE_EMPTY_TRANSCRIPT_MESSAGE: &str =
    "Не получилось уверенно разобрать голосовое. Попробуйте отправить его ещё раз или напишите задачу текстом.";

// ─── Coordinator ─────────────────────────────────────────────────────────────

/// Stateless coordinator for the voice-task creation flow.
///
/// Constructed per-request from the shared `bot` handle and `TelegramRuntime`
/// dependency container.  All session state lives in `state.creation_sessions`
/// and `state.assignee_selections`; the coordinator only orchestrates I/O and
/// delegates persistence to the application layer.
pub(crate) struct VoiceCreateCoordinator<'a> {
    bot: &'a Bot,
    state: &'a TelegramRuntime,
}

impl<'a> VoiceCreateCoordinator<'a> {
    pub fn new(bot: &'a Bot, state: &'a TelegramRuntime) -> Self {
        Self { bot, state }
    }

    /// Begins a new voice-create session: shows a processing indicator,
    /// transcribes the audio, stores the draft, and shows the confirmation screen.
    pub async fn start(
        &self,
        actor: &User,
        incoming_message: IncomingMessage,
    ) -> Result<(), teloxide::RequestError> {
        let chat_id = ChatId(incoming_message.chat_id);

        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::Processing,
            VOICE_PROCESSING_MESSAGE,
            InlineKeyboardMarkup::default(),
        )
        .await?;

        match self
            .state
            .create_task_use_case
            .transcribe_voice_message(&incoming_message)
            .await
        {
            Ok(transcript) => match NormalizedTranscript::from_raw(&transcript) {
                Ok(normalised) => {
                    let mut draft = VoiceTaskDraft::new(
                        incoming_message.source_message_key(),
                        normalised.text.clone(),
                    );
                    draft = draft.with_truncation(normalised.truncated);
                    self.state
                        .creation_sessions
                        .set_voice(chat_id.0, draft.clone())
                        .await;
                    self.state.assignee_selections.clear(chat_id.0).await;
                    self.show_confirmation(actor, chat_id, &draft, None).await
                }
                Err(_) => {
                    send_screen(
                        self.bot,
                        self.state,
                        chat_id,
                        ScreenDescriptor::QuickCreate,
                        VOICE_EMPTY_TRANSCRIPT_MESSAGE,
                        ui::create_menu_keyboard(),
                    )
                    .await
                }
            },
            Err(error) => {
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::QuickCreate,
                    &transcription_error_text(&error),
                    ui::create_menu_keyboard(),
                )
                .await
            }
        }
    }

    /// Routes an incoming message during an active voice session.
    ///
    /// - A new voice message restarts the flow from transcription.
    /// - A text message replaces the transcript or echoes the confirmation hint,
    ///   depending on the current `VoiceTaskStep`.
    pub async fn handle_message(
        &self,
        incoming_message: IncomingMessage,
        actor: &User,
        draft: VoiceTaskDraft,
    ) -> Result<(), teloxide::RequestError> {
        if matches!(&incoming_message.content, MessageContent::Voice { .. }) {
            return self.start(actor, incoming_message).await;
        }

        let chat_id = ChatId(incoming_message.chat_id);
        let Some(text) = incoming_message.text_payload().map(str::trim) else {
            return self
                .show_prompt_again(actor, chat_id, &draft, VOICE_TEXT_REQUIRED_MESSAGE)
                .await;
        };

        if text.is_empty() {
            return self
                .show_prompt_again(actor, chat_id, &draft, VOICE_TEXT_REQUIRED_MESSAGE)
                .await;
        }

        match draft.step {
            VoiceTaskStep::EditTranscript => {
                let updated = draft.replace_transcript(text.to_owned());
                self.state
                    .creation_sessions
                    .update_voice(chat_id.0, updated.clone())
                    .await;
                self.show_confirmation(
                    actor,
                    chat_id,
                    &updated,
                    Some(VOICE_TRANSCRIPT_UPDATED_NOTICE),
                )
                .await
            }
            VoiceTaskStep::Confirm => {
                // User sent text while on the confirmation screen — just remind them.
                let body = self.build_confirmation_body(actor, chat_id, &draft).await;
                let message = format!("{VOICE_CONFIRMATION_HINT}\n\n{body}");
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::VoiceCreate(VoiceTaskStep::Confirm),
                    &message,
                    ui::voice_confirmation_keyboard(),
                )
                .await
            }
        }
    }

    /// Transitions the session into `EditTranscript` mode and shows the edit prompt.
    pub async fn start_edit(&self, chat_id: ChatId) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Voice(draft)) = self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        let updated = draft.start_editing();
        self.state
            .creation_sessions
            .update_voice(chat_id.0, updated.clone())
            .await;

        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::VoiceCreate(VoiceTaskStep::EditTranscript),
            &ui::voice_edit_prompt(&updated.transcript),
            ui::voice_edit_keyboard(),
        )
        .await
    }

    /// Returns to the `Confirm` step after the user has finished editing.
    pub async fn return_to_confirmation(
        &self,
        actor: &User,
        chat_id: ChatId,
    ) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Voice(draft)) = self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        let updated = draft.return_to_confirmation();
        self.state
            .creation_sessions
            .update_voice(chat_id.0, updated.clone())
            .await;
        self.show_confirmation(actor, chat_id, &updated, None).await
    }

    /// Clears the session and returns the user to the CreateMenu screen.
    pub async fn cancel(&self, chat_id: ChatId) -> Result<(), teloxide::RequestError> {
        self.state.creation_sessions.clear(chat_id.0).await;
        self.state.assignee_selections.clear(chat_id.0).await;
        self.fallback_to_create_menu(chat_id).await
    }

    /// Submits the voice draft to the `create_task` use case.
    ///
    /// On success, clears the session and shows the creation result.
    /// On `ClarificationRequired`, stores the candidate list and shows the
    /// clarification keyboard so the user can pick the correct assignee.
    pub async fn submit(
        &self,
        actor: &User,
        chat_id: ChatId,
    ) -> Result<(), teloxide::RequestError> {
        let Some(CreationSession::Voice(draft)) = self.state.creation_sessions.get(chat_id.0).await
        else {
            return self.fallback_to_create_menu(chat_id).await;
        };

        let synthetic_message = build_voice_message(chat_id.0, actor, &draft);
        let preview_body = self.build_confirmation_body(actor, chat_id, &draft).await;

        match self
            .state
            .create_task_use_case
            .execute(synthetic_message.clone())
            .await
        {
            Ok(outcome @ TaskCreationOutcome::Created(_))
            | Ok(outcome @ TaskCreationOutcome::DuplicateFound(_)) => {
                self.state.creation_sessions.clear(chat_id.0).await;
                self.state.assignee_selections.clear(chat_id.0).await;
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::TaskCreationResult {
                        task_uid: Some(task_uid_from_outcome(&outcome)),
                    },
                    &ui::task_creation_text(&outcome),
                    ui::outcome_keyboard(&outcome),
                )
                .await
            }
            Ok(TaskCreationOutcome::ClarificationRequired(request)) => {
                self.state
                    .assignee_selections
                    .set_create(
                        chat_id.0,
                        synthetic_message,
                        clarification_candidate_ids(&request),
                        request.allow_unassigned,
                    )
                    .await;
                send_screen(
                    self.bot,
                    self.state,
                    chat_id,
                    ScreenDescriptor::VoiceCreate(VoiceTaskStep::Confirm),
                    &format!("{}\n\n{}", request.message, preview_body),
                    ui::clarification_keyboard(&request),
                )
                .await
            }
            Err(error) => send_error(self.bot, self.state, chat_id.0, error).await,
        }
    }

    // ─── Private helpers ──────────────────────────────────────────────────────

    async fn show_confirmation(
        &self,
        actor: &User,
        chat_id: ChatId,
        draft: &VoiceTaskDraft,
        notice: Option<&str>,
    ) -> Result<(), teloxide::RequestError> {
        let base = self.build_confirmation_body(actor, chat_id, draft).await;
        let mut sections: Vec<String> = Vec::new();
        if let Some(n) = notice {
            sections.push(n.to_owned());
        }
        if draft.truncated {
            // Surface the truncation explicitly — every time the draft is
            // rendered while in the clipped state — so the user cannot miss it.
            sections.push(
                "⚠️ Голосовое оказалось длинным, я использовал только начало. \
                 Проверьте, что все важные детали сохранились, или запишите заново."
                    .to_owned(),
            );
        }
        sections.push(base);
        let text = sections.join("\n\n");
        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::VoiceCreate(VoiceTaskStep::Confirm),
            &text,
            ui::voice_confirmation_keyboard(),
        )
        .await
    }

    async fn show_prompt_again(
        &self,
        actor: &User,
        chat_id: ChatId,
        draft: &VoiceTaskDraft,
        prefix: &str,
    ) -> Result<(), teloxide::RequestError> {
        let (body, keyboard) = match draft.step {
            VoiceTaskStep::Confirm => (
                self.build_confirmation_body(actor, chat_id, draft).await,
                ui::voice_confirmation_keyboard(),
            ),
            VoiceTaskStep::EditTranscript => (
                ui::voice_edit_prompt(&draft.transcript),
                ui::voice_edit_keyboard(),
            ),
        };
        send_screen(
            self.bot,
            self.state,
            chat_id,
            ScreenDescriptor::VoiceCreate(draft.step),
            &format!("{prefix}\n\n{body}"),
            keyboard,
        )
        .await
    }

    async fn build_confirmation_body(
        &self,
        actor: &User,
        chat_id: ChatId,
        draft: &VoiceTaskDraft,
    ) -> String {
        let preview_message = build_voice_message(chat_id.0, actor, draft);
        match self
            .state
            .create_task_use_case
            .preview_interpretation(&preview_message)
            .await
        {
            Ok(preview) => ui::voice_interpretation_text(&draft.transcript, &preview),
            Err(_) => ui::voice_confirmation_text(&draft.transcript),
        }
    }

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

// ─── Pure helpers (no Telegram I/O) ──────────────────────────────────────────

/// Constructs the synthetic `IncomingMessage` that the create-task use case
/// receives when the user submits a voice draft.
pub(crate) fn build_voice_message(
    chat_id: i64,
    actor: &User,
    draft: &VoiceTaskDraft,
) -> IncomingMessage {
    IncomingMessage {
        message_id: VOICE_SYNTHETIC_MESSAGE_ID,
        chat_id,
        sender_id: actor.telegram_id,
        sender_name: actor
            .full_name
            .clone()
            .unwrap_or_else(|| "Пользователь".to_owned()),
        sender_username: actor.telegram_username.clone(),
        content: MessageContent::Text {
            text: draft.transcript.clone(),
        },
        timestamp: chrono::Utc::now(),
        source_message_key_override: Some(draft.source_message_key.clone()),
        is_voice_origin: true,
    }
}

fn transcription_error_text(error: &AppError) -> String {
    match error.code() {
        "VOICE_TOO_LONG" => {
            "⏱ Голосовое слишком длинное. Запишите покороче или пришлите задачу текстом.".to_owned()
        }
        "VOICE_TOO_LARGE" => {
            "📦 Файл голосового слишком большой. Попробуйте более короткую запись.".to_owned()
        }
        "TRANSCRIPTION_EMPTY" => {
            "🔇 Не удалось уверенно разобрать речь. Попробуйте ещё раз или напишите текстом."
                .to_owned()
        }
        "CIRCUIT_BREAKER_OPEN" => {
            "🔌 Сервис распознавания временно недоступен. Попробуйте позже или напишите текстом."
                .to_owned()
        }
        _ => "⚠️ Не удалось обработать голосовое. Попробуйте ещё раз или напишите текстом."
            .to_owned(),
    }
}

fn task_uid_from_outcome(outcome: &TaskCreationOutcome) -> uuid::Uuid {
    match outcome {
        TaskCreationOutcome::Created(summary) | TaskCreationOutcome::DuplicateFound(summary) => {
            summary.task_uid
        }
        TaskCreationOutcome::ClarificationRequired(_) => unreachable!(),
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::build_voice_message;
    use crate::domain::user::{
        OnboardingState, User, UserRole, DEFAULT_QUIET_HOURS_END_MIN,
        DEFAULT_QUIET_HOURS_START_MIN, DEFAULT_USER_TIMEZONE,
    };
    use crate::presentation::telegram::drafts::VoiceTaskDraft;

    #[test]
    fn given_voice_draft_when_building_synthetic_message_then_preserves_original_source_key() {
        let actor = User {
            id: Some(7),
            telegram_id: 44,
            last_chat_id: Some(44),
            telegram_username: Some("leader".to_owned()),
            full_name: Some("Team Lead".to_owned()),
            first_name: Some("Team".to_owned()),
            last_name: Some("Lead".to_owned()),
            linked_employee_id: Some(11),
            is_employee: true,
            role: UserRole::User,
            onboarding_state: OnboardingState::Completed,
            onboarding_version: 0,
            timezone: DEFAULT_USER_TIMEZONE.to_owned(),
            quiet_hours_start_min: DEFAULT_QUIET_HOURS_START_MIN,
            quiet_hours_end_min: DEFAULT_QUIET_HOURS_END_MIN,
            deactivated_at: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let draft = VoiceTaskDraft::new(
            "telegram:99:123".to_owned(),
            "@ivanov подготовить релиз".to_owned(),
        );

        let message = build_voice_message(99, &actor, &draft);

        assert_eq!(
            message.source_message_key_override.as_deref(),
            Some("telegram:99:123")
        );
    }
}
