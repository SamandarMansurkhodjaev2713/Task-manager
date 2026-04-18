use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::application::use_cases::collect_stats::StatsScope;
use crate::presentation::telegram::callbacks::{TaskCardMode, TaskListOrigin};
use crate::presentation::telegram::drafts::{GuidedTaskStep, VoiceTaskStep};
use crate::presentation::telegram::interactions::TaskInteractionKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenDescriptor {
    Unknown,
    /// Transient screen shown while a slow async operation (e.g. voice transcription) is running.
    /// Always replaced by the next real screen — never displayed with interactive buttons.
    Processing,
    RegistrationLinking,
    MainMenu,
    Help,
    Settings,
    CreateMenu,
    QuickCreate,
    GuidedStep(GuidedTaskStep),
    VoiceCreate(VoiceTaskStep),
    TaskList(TaskListOrigin),
    TaskDetail {
        task_uid: Uuid,
        mode: TaskCardMode,
        origin: TaskListOrigin,
    },
    CancelConfirmation {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    TaskInteractionPrompt {
        task_uid: Uuid,
        origin: TaskListOrigin,
        kind: TaskInteractionKind,
    },
    TaskCreationResult {
        task_uid: Option<Uuid>,
    },
    DeliveryHelp {
        task_uid: Uuid,
        origin: TaskListOrigin,
    },
    Stats(StatsScope),
    SyncEmployeesResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveScreenState {
    pub message_id: i32,
    pub descriptor: ScreenDescriptor,
}

#[derive(Clone, Default)]
pub struct ActiveScreenStore {
    states: Arc<RwLock<HashMap<i64, ActiveScreenState>>>,
}

impl ActiveScreenStore {
    pub async fn get(&self, chat_id: i64) -> Option<ActiveScreenState> {
        self.states.read().await.get(&chat_id).cloned()
    }

    pub async fn set(&self, chat_id: i64, state: ActiveScreenState) {
        self.states.write().await.insert(chat_id, state);
    }

    pub async fn clear(&self, chat_id: i64) {
        self.states.write().await.remove(&chat_id);
    }

    pub async fn hydrate_if_missing(&self, chat_id: i64, message_id: i32) {
        let mut states = self.states.write().await;
        states.entry(chat_id).or_insert(ActiveScreenState {
            message_id,
            descriptor: ScreenDescriptor::Unknown,
        });
    }

    pub async fn is_stale(&self, chat_id: i64, message_id: i32) -> bool {
        self.get(chat_id)
            .await
            .map(|state| state.message_id != message_id)
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::{ActiveScreenState, ActiveScreenStore, ScreenDescriptor};

    #[tokio::test]
    async fn given_missing_state_when_hydrate_then_registers_unknown_screen() {
        let store = ActiveScreenStore::default();

        store.hydrate_if_missing(42, 100).await;

        assert_eq!(
            store.get(42).await,
            Some(ActiveScreenState {
                message_id: 100,
                descriptor: ScreenDescriptor::Unknown,
            })
        );
    }

    #[tokio::test]
    async fn given_current_screen_when_message_id_differs_then_detects_stale_state() {
        let store = ActiveScreenStore::default();
        store
            .set(
                7,
                ActiveScreenState {
                    message_id: 200,
                    descriptor: ScreenDescriptor::MainMenu,
                },
            )
            .await;

        assert!(store.is_stale(7, 150).await);
        assert!(!store.is_stale(7, 200).await);
    }
}
