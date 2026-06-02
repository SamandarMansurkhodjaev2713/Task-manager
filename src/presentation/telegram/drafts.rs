use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::presentation::telegram::callbacks::DraftEditField;

/// Sessions that have not been touched for longer than this are considered
/// stale and silently dropped on the next access.  30 minutes matches the
/// typical "user walked away mid-flow" pattern: long enough to not interrupt
/// normal use, short enough to free memory and prevent the FSM from resuming
/// with very old context that would confuse both parties.
pub const DRAFT_IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Clone)]
pub enum CreationSession {
    QuickCapture,
    Guided(GuidedTaskDraft),
    Voice(VoiceTaskDraft),
}

#[derive(Debug, Clone)]
pub struct GuidedTaskDraft {
    /// Stable identity for this draft submission; used as `source_message_key`
    /// so that retrying the same guided submit is idempotent (INSERT OR IGNORE).
    pub submission_key: Uuid,
    /// Raw assignee text entered by the user (first name, full name, or @username).
    /// Kept for display purposes and as the fallback when `resolved_employee_id` is absent.
    pub assignee: Option<String>,
    /// Employee ID confirmed during the early-resolution step.
    ///
    /// When `Some`, `submit()` calls `execute_with_assignee_decision(EmployeeId(id))`
    /// and skips the text-based fuzzy matcher entirely.  Set to `None` when the
    /// user re-edits the Assignee step (via `edit_field(Assignee)`) so that
    /// a changed input forces a fresh resolution.
    pub resolved_employee_id: Option<i64>,
    pub description: Option<String>,
    pub deadline: Option<String>,
    pub step: GuidedTaskStep,
}

#[derive(Debug, Clone)]
pub struct VoiceTaskDraft {
    pub source_message_key: String,
    pub transcript: String,
    pub step: VoiceTaskStep,
    /// `true` when the STT output was clipped to the token budget
    /// (see `NormalizedTranscript`) so the UI can surface a warning and
    /// offer "записать заново" as a primary CTA.
    pub truncated: bool,
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

/// A `CreationSession` tagged with its insertion time for lazy-expiry.
struct TimedSession {
    session: CreationSession,
    inserted_at: Instant,
}

impl TimedSession {
    fn new(session: CreationSession) -> Self {
        Self {
            session,
            inserted_at: Instant::now(),
        }
    }

    fn is_expired(&self) -> bool {
        self.inserted_at.elapsed() > DRAFT_IDLE_TIMEOUT
    }
}

#[derive(Clone, Default)]
pub struct CreationSessionStore {
    sessions: Arc<RwLock<HashMap<i64, TimedSession>>>,
}

impl GuidedTaskDraft {
    pub fn new() -> Self {
        Self {
            submission_key: Uuid::now_v7(),
            assignee: None,
            resolved_employee_id: None,
            description: None,
            deadline: None,
            step: GuidedTaskStep::Assignee,
        }
    }

    pub fn edit_field(&mut self, field: DraftEditField) {
        self.step = match field {
            DraftEditField::Assignee => {
                // Clear the pre-resolved employee so a changed assignee text
                // is not silently paired with the old resolved ID.
                self.resolved_employee_id = None;
                GuidedTaskStep::Assignee
            }
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
            truncated: false,
        }
    }

    /// Tag the draft as clipped so downstream UI can render a warning.
    pub fn with_truncation(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }

    pub fn start_editing(mut self) -> Self {
        self.step = VoiceTaskStep::EditTranscript;
        self
    }

    pub fn replace_transcript(mut self, transcript: String) -> Self {
        self.transcript = transcript;
        self.step = VoiceTaskStep::Confirm;
        // Manual edit supersedes the STT-clipping warning.
        self.truncated = false;
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
            .insert(chat_id, TimedSession::new(CreationSession::QuickCapture));
    }

    pub async fn set_guided(&self, chat_id: i64) {
        self.sessions.write().await.insert(
            chat_id,
            TimedSession::new(CreationSession::Guided(GuidedTaskDraft::new())),
        );
    }

    pub async fn set_voice(&self, chat_id: i64, draft: VoiceTaskDraft) {
        self.sessions
            .write()
            .await
            .insert(chat_id, TimedSession::new(CreationSession::Voice(draft)));
    }

    /// Returns the current session for `chat_id`, or `None` if absent **or
    /// expired**.  An expired session is silently removed so that the user
    /// starts a clean flow rather than resuming a stale one.
    pub async fn get(&self, chat_id: i64) -> Option<CreationSession> {
        // Fast path: read lock, return if present and fresh.
        {
            let guard = self.sessions.read().await;
            match guard.get(&chat_id) {
                Some(timed) if !timed.is_expired() => return Some(timed.session.clone()),
                None => return None,
                Some(_) => {} // expired — fall through to write-lock removal
            }
        }
        // Slow path: session exists but is expired — promote to write lock and evict.
        tracing::debug!(
            chat_id,
            "creation session expired; evicting stale FSM state"
        );
        self.sessions.write().await.remove(&chat_id);
        None
    }

    pub async fn update_guided(&self, chat_id: i64, draft: GuidedTaskDraft) {
        self.sessions
            .write()
            .await
            .insert(chat_id, TimedSession::new(CreationSession::Guided(draft)));
    }

    pub async fn update_voice(&self, chat_id: i64, draft: VoiceTaskDraft) {
        self.sessions
            .write()
            .await
            .insert(chat_id, TimedSession::new(CreationSession::Voice(draft)));
    }

    pub async fn clear(&self, chat_id: i64) {
        self.sessions.write().await.remove(&chat_id);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::presentation::telegram::callbacks::DraftEditField;

    use super::{
        CreationSessionStore, GuidedTaskDraft, GuidedTaskStep, TimedSession, VoiceTaskDraft,
        VoiceTaskStep,
    };

    // ── FSM idle timeout ─────────────────────────────────────────────────────

    #[tokio::test]
    async fn given_fresh_session_when_get_then_returns_session() {
        let store = CreationSessionStore::default();
        store.set_quick_capture(1).await;
        let result = store.get(1).await;
        assert!(
            result.is_some(),
            "fresh session must be returned from store"
        );
    }

    #[tokio::test]
    async fn given_no_session_when_get_then_returns_none() {
        let store = CreationSessionStore::default();
        let result = store.get(999).await;
        assert!(result.is_none(), "absent session must return None");
    }

    #[tokio::test]
    async fn given_expired_session_when_get_then_returns_none_and_evicts() {
        // Build a TimedSession with a very short timeout by back-dating its
        // `inserted_at` field.  We do this directly to avoid sleeping in tests.
        use super::{CreationSession, DRAFT_IDLE_TIMEOUT};
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::RwLock;

        // Construct an already-expired timed session by shifting inserted_at back.
        let expired = TimedSession {
            session: CreationSession::QuickCapture,
            // Simulate "inserted 31 minutes ago"
            inserted_at: std::time::Instant::now()
                .checked_sub(DRAFT_IDLE_TIMEOUT + Duration::from_secs(60))
                .expect("backdate must not underflow"),
        };

        let store = CreationSessionStore {
            sessions: Arc::new(RwLock::new(HashMap::from([(42_i64, expired)]))),
        };

        // get() must evict and return None.
        let result = store.get(42).await;
        assert!(result.is_none(), "expired session must not be returned");

        // The session must also be removed from the map.
        assert!(
            store.sessions.read().await.get(&42).is_none(),
            "expired session must be evicted from the store"
        );
    }

    #[tokio::test]
    async fn given_cleared_session_when_get_then_returns_none() {
        let store = CreationSessionStore::default();
        store.set_guided(5).await;
        store.clear(5).await;
        let result = store.get(5).await;
        assert!(result.is_none(), "cleared session must not be returned");
    }

    // ── GuidedTaskDraft ──────────────────────────────────────────────────────

    #[test]
    fn given_draft_with_resolved_employee_when_edit_assignee_then_clears_resolved_id() {
        let mut draft = GuidedTaskDraft::new();
        draft.resolved_employee_id = Some(42);
        draft.step = GuidedTaskStep::Description;

        draft.edit_field(DraftEditField::Assignee);

        assert_eq!(
            draft.resolved_employee_id, None,
            "re-editing the assignee step must clear the pre-resolved employee ID"
        );
        assert_eq!(draft.step, GuidedTaskStep::Assignee);
    }

    #[test]
    fn given_draft_with_resolved_employee_when_edit_description_then_preserves_resolved_id() {
        let mut draft = GuidedTaskDraft::new();
        draft.resolved_employee_id = Some(42);
        draft.step = GuidedTaskStep::Confirm;

        draft.edit_field(DraftEditField::Description);

        assert_eq!(
            draft.resolved_employee_id,
            Some(42),
            "editing a non-assignee field must not touch resolved_employee_id"
        );
        assert_eq!(draft.step, GuidedTaskStep::Description);
    }

    // ── VoiceTaskDraft ───────────────────────────────────────────────────────

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
