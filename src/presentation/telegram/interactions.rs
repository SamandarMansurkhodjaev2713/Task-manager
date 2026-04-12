use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::presentation::telegram::callbacks::TaskListOrigin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskInteractionKind {
    Comment,
    Blocker,
    Reassign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskInteractionSession {
    pub task_uid: Uuid,
    pub origin: TaskListOrigin,
    pub kind: TaskInteractionKind,
}

#[derive(Clone, Default)]
pub struct TaskInteractionSessionStore {
    sessions: Arc<RwLock<HashMap<i64, TaskInteractionSession>>>,
}

impl TaskInteractionSessionStore {
    pub async fn set(&self, chat_id: i64, session: TaskInteractionSession) {
        self.sessions.write().await.insert(chat_id, session);
    }

    pub async fn get(&self, chat_id: i64) -> Option<TaskInteractionSession> {
        self.sessions.read().await.get(&chat_id).copied()
    }

    pub async fn clear(&self, chat_id: i64) {
        self.sessions.write().await.remove(&chat_id);
    }
}
