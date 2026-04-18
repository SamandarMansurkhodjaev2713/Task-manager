use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingRegistrationLink {
    pub candidate_ids: Vec<i64>,
    pub allow_continue_unlinked: bool,
}

#[derive(Clone, Default)]
pub struct PendingRegistrationLinkStore {
    states: Arc<RwLock<HashMap<i64, PendingRegistrationLink>>>,
}

impl PendingRegistrationLinkStore {
    pub async fn get(&self, chat_id: i64) -> Option<PendingRegistrationLink> {
        self.states.read().await.get(&chat_id).cloned()
    }

    pub async fn set(&self, chat_id: i64, candidate_ids: Vec<i64>, allow_continue_unlinked: bool) {
        self.states.write().await.insert(
            chat_id,
            PendingRegistrationLink {
                candidate_ids,
                allow_continue_unlinked,
            },
        );
    }

    pub async fn clear(&self, chat_id: i64) {
        self.states.write().await.remove(&chat_id);
    }
}
