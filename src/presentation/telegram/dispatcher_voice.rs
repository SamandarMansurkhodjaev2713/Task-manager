use teloxide::types::ChatId;
use teloxide::Bot;

use crate::application::dto::task_views::TaskCreationOutcome;
use crate::domain::message::{IncomingMessage, MessageContent};
use crate::domain::user::User;
use crate::presentation::telegram::active_screens::ScreenDescriptor;
use crate::presentation::telegram::drafts::{CreationSession, VoiceTaskDraft, VoiceTaskStep};
use crate::presentation::telegram::ui;

use super::dispatcher_transport::{send_error, send_screen};
use super::TelegramRuntime;

const VOICE_SYNTHETIC_MESSAGE_ID: i32 = -1;
const VOICE_TEXT_REQUIRED_MESSAGE: &str =
    "Нужен один короткий текст сообщением. Я заменю им расшифровку и снова покажу подтверждение.";
const VOICE_TRANSCRIPT_UPDATED_NOTICE: &str = "✏️ Текст обновлён. Проверьте финальную версию.";
const VOICE_CONFIRMATION_HINT: &str =
    "Если хотите изменить формулировку, нажмите «Исправить текст» или отправьте новое голосовое.";

pub(crate) async fn start_voice_create(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ChatId(incoming_message.chat_id);
    match state
        .create_task_use_case
        .transcribe_voice_message(&incoming_message)
        .await
    {
        Ok(transcript) => {
            let normalized_transcript = transcript.trim().to_owned();
            if normalized_transcript.is_empty() {
                return send_screen(
                    bot,
                    state,
                    chat_id,
                    ScreenDescriptor::QuickCreate,
                    "Не получилось уверенно разобрать голосовое. Попробуйте отправить его ещё раз или напишите задачу текстом.",
                    ui::create_menu_keyboard(),
                )
                .await;
            }

            let draft =
                VoiceTaskDraft::new(incoming_message.source_message_key(), normalized_transcript);
            state
                .creation_sessions
                .set_voice(chat_id.0, draft.clone())
                .await;
            show_voice_confirmation(bot, state, chat_id, &draft, None).await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

pub(crate) async fn handle_voice_creation_session_message(
    bot: &Bot,
    state: &TelegramRuntime,
    incoming_message: IncomingMessage,
    draft: VoiceTaskDraft,
) -> Result<(), teloxide::RequestError> {
    if matches!(&incoming_message.content, MessageContent::Voice { .. }) {
        return start_voice_create(bot, state, incoming_message).await;
    }

    let chat_id = ChatId(incoming_message.chat_id);
    let Some(text) = incoming_message.text_payload().map(str::trim) else {
        return show_voice_prompt_again(bot, state, chat_id, &draft, VOICE_TEXT_REQUIRED_MESSAGE)
            .await;
    };

    if text.is_empty() {
        return show_voice_prompt_again(bot, state, chat_id, &draft, VOICE_TEXT_REQUIRED_MESSAGE)
            .await;
    }

    match draft.step {
        VoiceTaskStep::EditTranscript => {
            let updated_draft = draft.replace_transcript(text.to_owned());
            state
                .creation_sessions
                .update_voice(chat_id.0, updated_draft.clone())
                .await;
            show_voice_confirmation(
                bot,
                state,
                chat_id,
                &updated_draft,
                Some(VOICE_TRANSCRIPT_UPDATED_NOTICE),
            )
            .await
        }
        VoiceTaskStep::Confirm => {
            let message = format!(
                "{VOICE_CONFIRMATION_HINT}\n\n{}",
                ui::voice_confirmation_text(&draft.transcript)
            );
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::VoiceCreate(VoiceTaskStep::Confirm),
                &message,
                ui::voice_confirmation_keyboard(),
            )
            .await
        }
    }
}

pub(crate) async fn start_voice_transcript_edit(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Voice(draft)) = state.creation_sessions.get(chat_id.0).await else {
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

    let updated_draft = draft.start_editing();
    state
        .creation_sessions
        .update_voice(chat_id.0, updated_draft.clone())
        .await;

    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::VoiceCreate(VoiceTaskStep::EditTranscript),
        &ui::voice_edit_prompt(&updated_draft.transcript),
        ui::voice_edit_keyboard(),
    )
    .await
}

pub(crate) async fn return_to_voice_confirmation(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Voice(draft)) = state.creation_sessions.get(chat_id.0).await else {
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

    let updated_draft = draft.return_to_confirmation();
    state
        .creation_sessions
        .update_voice(chat_id.0, updated_draft.clone())
        .await;
    show_voice_confirmation(bot, state, chat_id, &updated_draft, None).await
}

pub(crate) async fn cancel_voice_create(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    state.creation_sessions.clear(chat_id.0).await;
    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::CreateMenu,
        &ui::create_menu_text(),
        ui::create_menu_keyboard(),
    )
    .await
}

pub(crate) async fn submit_voice_draft(
    bot: &Bot,
    state: &TelegramRuntime,
    actor: &User,
    chat_id: ChatId,
) -> Result<(), teloxide::RequestError> {
    let Some(CreationSession::Voice(draft)) = state.creation_sessions.get(chat_id.0).await else {
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

    let synthetic_message = build_voice_message(chat_id.0, actor, &draft);
    match state.create_task_use_case.execute(synthetic_message).await {
        Ok(outcome @ TaskCreationOutcome::Created(_))
        | Ok(outcome @ TaskCreationOutcome::DuplicateFound(_)) => {
            state.creation_sessions.clear(chat_id.0).await;
            send_screen(
                bot,
                state,
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
            let text = format!(
                "ℹ️ {}\n\n{}",
                request.message,
                ui::voice_confirmation_text(&draft.transcript)
            );
            send_screen(
                bot,
                state,
                chat_id,
                ScreenDescriptor::VoiceCreate(VoiceTaskStep::Confirm),
                &text,
                ui::voice_confirmation_keyboard(),
            )
            .await
        }
        Err(error) => send_error(bot, chat_id.0, error).await,
    }
}

fn build_voice_message(chat_id: i64, actor: &User, draft: &VoiceTaskDraft) -> IncomingMessage {
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

async fn show_voice_confirmation(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    draft: &VoiceTaskDraft,
    notice: Option<&str>,
) -> Result<(), teloxide::RequestError> {
    let base_text = ui::voice_confirmation_text(&draft.transcript);
    let text = match notice {
        Some(notice) => format!("{notice}\n\n{base_text}"),
        None => base_text,
    };

    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::VoiceCreate(VoiceTaskStep::Confirm),
        &text,
        ui::voice_confirmation_keyboard(),
    )
    .await
}

async fn show_voice_prompt_again(
    bot: &Bot,
    state: &TelegramRuntime,
    chat_id: ChatId,
    draft: &VoiceTaskDraft,
    prefix_message: &str,
) -> Result<(), teloxide::RequestError> {
    let body = match draft.step {
        VoiceTaskStep::Confirm => ui::voice_confirmation_text(&draft.transcript),
        VoiceTaskStep::EditTranscript => ui::voice_edit_prompt(&draft.transcript),
    };
    let keyboard = match draft.step {
        VoiceTaskStep::Confirm => ui::voice_confirmation_keyboard(),
        VoiceTaskStep::EditTranscript => ui::voice_edit_keyboard(),
    };

    send_screen(
        bot,
        state,
        chat_id,
        ScreenDescriptor::VoiceCreate(draft.step),
        &format!("{prefix_message}\n\n{body}"),
        keyboard,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::build_voice_message;
    use crate::domain::user::{User, UserRole};
    use crate::presentation::telegram::drafts::VoiceTaskDraft;

    #[test]
    fn given_voice_draft_when_building_synthetic_message_then_preserves_original_source_key() {
        let actor = User {
            id: Some(7),
            telegram_id: 44,
            last_chat_id: Some(44),
            telegram_username: Some("leader".to_owned()),
            full_name: Some("Team Lead".to_owned()),
            is_employee: false,
            role: UserRole::User,
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
