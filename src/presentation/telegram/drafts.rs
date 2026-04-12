use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::presentation::telegram::callbacks::DraftEditField;

#[derive(Debug, Clone)]
pub enum CreationSession {
    QuickCapture,
    Guided(GuidedTaskDraft),
}

#[derive(Debug, Clone)]
pub struct GuidedTaskDraft {
    pub submission_key: Uuid,
    pub assignee: Option<String>,
    pub description: Option<String>,
    pub deadline: Option<String>,
    pub step: GuidedTaskStep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuidedTaskStep {
    Assignee,
    Description,
    Deadline,
    Confirm,
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

    pub async fn get(&self, chat_id: i64) -> Option<CreationSession> {
        self.sessions.read().await.get(&chat_id).cloned()
    }

    pub async fn update_guided(&self, chat_id: i64, draft: GuidedTaskDraft) {
        self.sessions
            .write()
            .await
            .insert(chat_id, CreationSession::Guided(draft));
    }

    pub async fn clear(&self, chat_id: i64) {
        self.sessions.write().await.remove(&chat_id);
    }
}
