use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::domain::message::IncomingMessage;
use crate::presentation::telegram::callbacks::TaskListOrigin;

#[derive(Debug, Clone)]
pub enum PendingAssigneeSelection {
    Create(TaskCreationAssigneeSelection),
    Reassign(TaskReassignAssigneeSelection),
}

#[derive(Debug, Clone)]
pub struct TaskCreationAssigneeSelection {
    pub message: IncomingMessage,
    pub candidate_employee_ids: Vec<i64>,
    pub allow_unassigned: bool,
}

#[derive(Debug, Clone)]
pub struct TaskReassignAssigneeSelection {
    pub task_uid: Uuid,
    pub origin: TaskListOrigin,
    pub original_query: String,
    pub candidate_employee_ids: Vec<i64>,
}

#[derive(Clone, Default)]
pub struct PendingAssigneeSelectionStore {
    selections: Arc<RwLock<HashMap<i64, PendingAssigneeSelection>>>,
}

impl PendingAssigneeSelectionStore {
    pub async fn get(&self, chat_id: i64) -> Option<PendingAssigneeSelection> {
        self.selections.read().await.get(&chat_id).cloned()
    }

    pub async fn set_create(
        &self,
        chat_id: i64,
        message: IncomingMessage,
        candidate_employee_ids: Vec<i64>,
        allow_unassigned: bool,
    ) {
        self.selections.write().await.insert(
            chat_id,
            PendingAssigneeSelection::Create(TaskCreationAssigneeSelection {
                message,
                candidate_employee_ids,
                allow_unassigned,
            }),
        );
    }

    pub async fn set_reassign(
        &self,
        chat_id: i64,
        task_uid: Uuid,
        origin: TaskListOrigin,
        original_query: String,
        candidate_employee_ids: Vec<i64>,
    ) {
        self.selections.write().await.insert(
            chat_id,
            PendingAssigneeSelection::Reassign(TaskReassignAssigneeSelection {
                task_uid,
                origin,
                original_query,
                candidate_employee_ids,
            }),
        );
    }

    pub async fn clear(&self, chat_id: i64) {
        self.selections.write().await.remove(&chat_id);
    }
}
