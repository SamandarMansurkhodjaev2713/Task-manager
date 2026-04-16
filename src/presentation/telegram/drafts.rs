use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::presentation::telegram::callbacks::DraftEditField;

#[derive(Debug, Clone)]
pub enum CreationSession {
    QuickCapture,
    Guided(GuidedTaskDraft),
    Voice(VoiceTaskDraft),
}

#[derive(Debug, Clone)]
pub struct GuidedTaskDraft {
    pub submission_key: Uuid,
    pub assignee: Option<String>,
    pub description: Option<String>,
    pub deadline: Option<String>,
    pub step: GuidedTaskStep,
}

#[derive(Debug, Clone)]
pub struct VoiceTaskDraft {
    pub source_message_key: String,
    pub transcript: String,
    pub step: VoiceTaskStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidedTaskStep {
    Assignee,
    Description,
    Deadline,
    Confirm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceTaskStep {
    Confirm,
    EditTranscript,
}

#[derive(Clone, Default)]
pub struct CreationSessionStore {
    sessions: Arc<RwLock<HashMap<i64, CreationSession>>>,
}

impl GuidedTaskDraft {
    pub fn new() -> Self {
        Self {
            submission_key: Uuid::now_v7(),
            assignee: None,
            description: None,
            deadline: None,
            step: GuidedTaskStep::Assignee,
        }
    }

    pub fn edit_field(&mut self, field: DraftEditField) {
        self.step = match field {
            DraftEditField::Assignee => GuidedTaskStep::Assignee,
            DraftEditField::Description => GuidedTaskStep::Description,
            DraftEditField::Deadline => GuidedTaskStep::Deadline,
        };
    }
}

impl Default for GuidedTaskDraft {
    fn default() -> Self {
        Self::new()
    }
}

impl VoiceTaskDraft {
    pub fn new(source_message_key: String, transcript: String) -> Self {
        Self {
            source_message_key,
            transcript,
            step: VoiceTaskStep::Confirm,
        }
    }

    pub fn start_editing(mut self) -> Self {
        self.step = VoiceTaskStep::EditTranscript;
        self
    }

    pub fn replace_transcript(mut self, transcript: String) -> Self {
        self.transcript = transcript;
        self.step = VoiceTaskStep::Confirm;
        self
    }

    pub fn return_to_confirmation(mut self) -> Self {
        self.step = VoiceTaskStep::Confirm;
        self
    }
}

impl CreationSessionStore {
    pub async fn set_quick_capture(&self, chat_id: i64) {
        self.sessions
            .write()
            .await
            .insert(chat_id, CreationSession::QuickCapture);
    }

    pub async fn set_guided(&self, chat_id: i64) {
        self.sessions
            .write()
            .await
            .insert(chat_id, CreationSession::Guided(GuidedTaskDraft::new()));
    }

    pub async fn set_voice(&self, chat_id: i64, draft: VoiceTaskDraft) {
        self.sessions
            .write()
            .await
            .insert(chat_id, CreationSession::Voice(draft));
    }

    pub async fn get(&self, chat_id: i64) -> Option<CreationSession> {
        self.sessions.read().await.get(&chat_id).cloned()
    }

    pub async fn update_guided(&self, chat_id: i64, draft: GuidedTaskDraft) {
        self.sessions
            .write()
            .await
            .insert(chat_id, CreationSession::Guided(draft));
    }

    pub async fn update_voice(&self, chat_id: i64, draft: VoiceTaskDraft) {
        self.sessions
            .write()
            .await
            .insert(chat_id, CreationSession::Voice(draft));
    }

    pub async fn clear(&self, chat_id: i64) {
        self.sessions.write().await.remove(&chat_id);
    }
}

#[cfg(test)]
mod tests {
    use super::{VoiceTaskDraft, VoiceTaskStep};

    #[test]
    fn given_voice_draft_when_start_editing_then_switches_to_edit_mode() {
        let draft = VoiceTaskDraft::new("telegram:1:10".to_owned(), "подготовить релиз".to_owned());

        let updated = draft.start_editing();

        assert_eq!(updated.step, VoiceTaskStep::EditTranscript);
    }

    #[test]
    fn given_voice_draft_when_replace_transcript_then_updates_text_and_returns_to_confirm() {
        let draft =
            VoiceTaskDraft::new("telegram:1:10".to_owned(), "черновик".to_owned()).start_editing();

        let updated = draft.replace_transcript("финальный текст задачи".to_owned());

        assert_eq!(updated.transcript, "финальный текст задачи");
        assert_eq!(updated.step, VoiceTaskStep::Confirm);
    }
}
