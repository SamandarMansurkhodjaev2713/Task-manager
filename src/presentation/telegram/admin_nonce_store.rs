//! In-memory nonce store for confirming destructive admin actions.
//!
//! # Why a nonce at all?
//!
//! The in-Telegram admin panel exposes destructive operations (role change,
//! deactivate / reactivate).  Without an out-of-band confirmation the UI is
//! a single button press away from an irreversible state transition, and a
//! scrollback click — or a replay of a stale `chat.id`/`message.id`
//! callback — could trigger it by accident.  The nonce adds:
//!
//! * **Two-step commit**: every action produces a one-time token, the user
//!   must press an explicit *"Подтвердить"* button to spend it.
//! * **Actor binding**: the nonce is bound to the admin who generated it; a
//!   pasted/leaked callback cannot be redeemed by another user.
//! * **Short TTL**: stale buttons from prior sessions expire automatically.
//! * **Single-use**: calling [`AdminNonceStore::consume`] removes the entry
//!   so the same callback cannot be replayed.
//!
//! # Why in-memory?
//!
//! The audit *outcome* of every admin action is persisted via the admin
//! audit log, so a restart losing pending nonces only costs the user a
//! re-click.  In-memory storage avoids an extra SQL round-trip and keeps
//! the code simple; a Redis-backed implementation can be swapped in
//! behind [`AdminNonceStore`] without touching callers.

use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use uuid::Uuid;

use crate::presentation::telegram::callbacks::AdminRoleOption;

/// A pending admin action awaiting confirmation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingAdminAction {
    ChangeRole {
        target_user_id: i64,
        target_telegram_id: i64,
        display_name: String,
        next_role: AdminRoleOption,
    },
    Deactivate {
        target_user_id: i64,
        target_telegram_id: i64,
        display_name: String,
    },
    Reactivate {
        target_user_id: i64,
        target_telegram_id: i64,
        display_name: String,
    },
}

impl PendingAdminAction {
    pub fn target_user_id(&self) -> i64 {
        match self {
            Self::ChangeRole { target_user_id, .. }
            | Self::Deactivate { target_user_id, .. }
            | Self::Reactivate { target_user_id, .. } => *target_user_id,
        }
    }
}

#[derive(Debug, Clone)]
struct NonceEntry {
    actor_user_id: i64,
    action: PendingAdminAction,
    expires_at: Instant,
}

/// Thread-safe, process-local store of pending admin confirmations.
#[derive(Debug, Clone)]
pub struct AdminNonceStore {
    inner: Arc<Mutex<HashMap<String, NonceEntry>>>,
    ttl: Duration,
}

impl AdminNonceStore {
    pub fn new(ttl_seconds: NonZeroU32) -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            ttl: Duration::from_secs(u64::from(ttl_seconds.get())),
        }
    }

    /// Creates a nonce binding `(actor, action)`.  Returns the nonce, which
    /// should be embedded in the confirmation callback.
    pub fn issue(&self, actor_user_id: i64, action: PendingAdminAction) -> String {
        let nonce = Self::generate_nonce();
        let entry = NonceEntry {
            actor_user_id,
            action,
            expires_at: Instant::now() + self.ttl,
        };
        let mut guard = self.inner.lock().expect("AdminNonceStore mutex poisoned");
        Self::sweep_expired(&mut guard);
        guard.insert(nonce.clone(), entry);
        nonce
    }

    /// Attempts to redeem `nonce` on behalf of `actor_user_id`.  The entry is
    /// removed atomically — callers only see `Ok(action)` if this invocation
    /// is the one that claimed it.  Returns the reason on failure so the UI
    /// can show a helpful error instead of silently doing nothing.
    pub fn consume(
        &self,
        actor_user_id: i64,
        nonce: &str,
    ) -> Result<PendingAdminAction, NonceError> {
        let mut guard = self.inner.lock().expect("AdminNonceStore mutex poisoned");
        Self::sweep_expired(&mut guard);
        let Some(entry) = guard.remove(nonce) else {
            return Err(NonceError::NotFound);
        };
        if entry.actor_user_id != actor_user_id {
            // Do not re-insert: this nonce is now burned.  A mis-directed
            // click should still make the original issuer re-confirm to
            // prevent lateral attacks.
            return Err(NonceError::WrongActor);
        }
        Ok(entry.action)
    }

    /// Discards the nonce without error, used by the explicit "Cancel"
    /// button or when a newer nonce supersedes an old one.
    pub fn discard(&self, nonce: &str) {
        let mut guard = self.inner.lock().expect("AdminNonceStore mutex poisoned");
        guard.remove(nonce);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        let guard = self.inner.lock().expect("AdminNonceStore mutex poisoned");
        guard.len()
    }

    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn sweep_expired(guard: &mut HashMap<String, NonceEntry>) {
        let now = Instant::now();
        guard.retain(|_, entry| entry.expires_at > now);
    }

    fn generate_nonce() -> String {
        // v7 UUID mixes a 48-bit timestamp with 74 bits of randomness; we
        // strip dashes to save callback-data bytes.  Uniqueness is enforced
        // both by the store and by the RNG entropy, so collisions on the
        // 120-bit space are not a practical concern.
        Uuid::now_v7().simple().to_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonceError {
    NotFound,
    WrongActor,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_action() -> PendingAdminAction {
        PendingAdminAction::ChangeRole {
            target_user_id: 10,
            target_telegram_id: 100,
            display_name: "Иван Иванов".to_owned(),
            next_role: AdminRoleOption::Manager,
        }
    }

    #[test]
    fn given_issued_nonce_when_consumed_by_actor_then_returns_action() {
        let store = AdminNonceStore::new(NonZeroU32::new(60).unwrap());
        let nonce = store.issue(42, sample_action());

        let result = store.consume(42, &nonce);

        assert_eq!(result, Ok(sample_action()));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn given_issued_nonce_when_consumed_by_other_actor_then_rejected_and_burned() {
        let store = AdminNonceStore::new(NonZeroU32::new(60).unwrap());
        let nonce = store.issue(42, sample_action());

        let result = store.consume(99, &nonce);

        assert_eq!(result, Err(NonceError::WrongActor));
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn given_unknown_nonce_when_consumed_then_returns_not_found() {
        let store = AdminNonceStore::new(NonZeroU32::new(60).unwrap());

        let result = store.consume(1, "deadbeef");

        assert_eq!(result, Err(NonceError::NotFound));
    }

    #[test]
    fn given_consumed_nonce_when_consumed_again_then_returns_not_found() {
        let store = AdminNonceStore::new(NonZeroU32::new(60).unwrap());
        let nonce = store.issue(42, sample_action());
        let _ = store.consume(42, &nonce);

        let result = store.consume(42, &nonce);

        assert_eq!(result, Err(NonceError::NotFound));
    }

    #[test]
    fn given_discarded_nonce_when_consumed_then_returns_not_found() {
        let store = AdminNonceStore::new(NonZeroU32::new(60).unwrap());
        let nonce = store.issue(42, sample_action());

        store.discard(&nonce);
        let result = store.consume(42, &nonce);

        assert_eq!(result, Err(NonceError::NotFound));
    }

    #[test]
    fn given_nonce_when_ttl_elapses_then_it_is_expired() {
        let store = AdminNonceStore::new(NonZeroU32::new(1).unwrap());
        let nonce = store.issue(42, sample_action());
        std::thread::sleep(Duration::from_millis(1_100));

        let result = store.consume(42, &nonce);

        assert_eq!(result, Err(NonceError::NotFound));
    }
}
