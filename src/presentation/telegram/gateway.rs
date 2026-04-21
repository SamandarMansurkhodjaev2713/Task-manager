//! Per-chat update gateway: serializes Telegram updates by chat id, de-duplicates
//! retried webhooks, and enforces a single outbound UX effect per update.
//!
//! The gateway is intentionally the *only* place in the presentation layer that
//! holds a long-lived per-chat mutex.  Every message or callback handler first
//! acquires a permit from [`ChatSerializer`] (see [`UpdateGuard`]) so that two
//! messages from the same chat cannot race through the business pipeline at the
//! same time.  Updates from *different* chats are still processed fully in
//! parallel — the lock map is keyed by `chat_id`.
//!
//! ## Why a barrier is needed on top of the mutex
//!
//! The per-chat mutex alone is not enough to prevent the *double-reply* bug we
//! saw on the onboarding screen: one update can traverse two orthogonal code
//! paths (onboarding gate → create-task dispatcher) in strict sequence, and
//! each path historically felt free to render a screen of its own.  The
//! [`UxBarrier`] carried inside [`UpdateGuard`] solves that at the transport
//! level: the first [`crate::presentation::telegram::dispatcher_transport`]
//! call that reaches the user (send_screen / send_fresh_screen / send_error)
//! consumes the barrier; every subsequent call within the same update becomes
//! a tracing::warn no-op.  This is defence-in-depth: the dispatcher-level fix
//! still returns early after the onboarding gate, but even if a future handler
//! regresses, the user will never see two competing messages again.
//!
//! ## Invariants (verified by tests in this module)
//!
//! * The barrier is scoped to *one* update; dropping [`UpdateGuard`] releases
//!   both the mutex and the barrier so the next update starts with a fresh one.
//! * `UxBarrier::try_consume` is atomic (compare-and-swap on `AtomicBool`) so
//!   two concurrent tasks sharing the same guard cannot both succeed.
//! * `UpdateDedup::observe` is a bounded LRU keyed by `(chat_id, message_id)`
//!   for messages and `(chat_id, callback_id)` for callbacks — Telegram reuses
//!   the same identifiers on webhook retries, so duplicates are rejected
//!   without touching the business layer.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use metrics::{counter, histogram};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

use std::sync::atomic::{AtomicBool, Ordering};

/// How many `(chat_id, dedup_key)` fingerprints we remember.  Telegram's
/// webhook retry window is a few minutes — 2 048 entries is more than enough
/// for realistic traffic and still fits in a few hundred kilobytes.
const UPDATE_DEDUP_CAPACITY: usize = 2_048;

/// Serializes updates per `chat_id` without serializing the whole bot.
///
/// Cloning a [`ChatSerializer`] is cheap: all state lives behind an `Arc`.
#[derive(Clone, Default)]
pub struct ChatSerializer {
    inner: Arc<RwLock<HashMap<i64, Arc<Mutex<()>>>>>,
}

impl ChatSerializer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Acquires the per-chat lock, records metrics and returns an
    /// [`UpdateGuard`] whose lifetime is the entire update handling.
    pub async fn acquire(&self, chat_id: i64) -> UpdateGuard {
        let lock = self.obtain_lock(chat_id).await;
        let started = Instant::now();
        let guard = lock.lock_owned().await;
        let waited_ms = started.elapsed().as_secs_f64() * 1_000.0;
        histogram!("gateway_lock_wait_ms").record(waited_ms);
        counter!("gateway_updates_entered_total").increment(1);

        UpdateGuard {
            _mutex_guard: guard,
            barrier: UxBarrier::new(),
        }
    }

    async fn obtain_lock(&self, chat_id: i64) -> Arc<Mutex<()>> {
        {
            let read = self.inner.read().await;
            if let Some(existing) = read.get(&chat_id) {
                return existing.clone();
            }
        }
        let mut write = self.inner.write().await;
        write
            .entry(chat_id)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

/// RAII guard held for the whole lifetime of one update.  Carries both the
/// per-chat mutex permit and the per-update UX barrier.  When the guard is
/// dropped, the mutex is released and the barrier (which is owned by the guard)
/// is no longer reachable, so the next update starts fresh.
pub struct UpdateGuard {
    // Keeping an owned mutex guard lets us hand out clones of `UxBarrier`
    // without lifetime gymnastics while still guaranteeing release at drop.
    _mutex_guard: OwnedMutexGuard<()>,
    barrier: UxBarrier,
}

impl UpdateGuard {
    /// Returns a clonable handle that the transport layer uses to gate the
    /// *single* outbound UX effect allowed per update.
    pub fn barrier(&self) -> UxBarrier {
        self.barrier.clone()
    }
}

/// One-shot token that may be "consumed" exactly once per update.
///
/// A clone of the barrier is stashed on every `IncomingMessage`-scoped
/// operation so that every `send_screen` / `send_fresh_screen` / `send_error`
/// call can ask the barrier *may I render now?* before actually hitting the
/// Telegram API.
#[derive(Clone)]
pub struct UxBarrier {
    flag: Arc<AtomicBool>,
}

impl UxBarrier {
    pub fn new() -> Self {
        Self {
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Attempts to consume the barrier.  Returns `true` iff this call is the
    /// first UX effect for the current update — in that case the caller is
    /// free to render.  Subsequent calls return `false` and MUST be no-ops.
    pub fn try_consume(&self) -> bool {
        let consumed_previously = self
            .flag
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err();
        if consumed_previously {
            counter!("gateway_ux_duplicate_suppressed_total").increment(1);
        }
        !consumed_previously
    }

    /// Returns `true` when this barrier has already produced its one UX effect.
    pub fn is_spent(&self) -> bool {
        self.flag.load(Ordering::Acquire)
    }
}

impl Default for UxBarrier {
    fn default() -> Self {
        Self::new()
    }
}

/// A small bounded LRU for `(chat_id, dedup_key)` pairs.  We only need to
/// reject duplicated webhook deliveries, not persist anything across restarts,
/// so a single `VecDeque` + `HashMap` is enough.
#[derive(Clone, Default)]
pub struct UpdateDedup {
    inner: Arc<Mutex<UpdateDedupInner>>,
}

#[derive(Default)]
struct UpdateDedupInner {
    seen: HashMap<DedupKey, ()>,
    order: VecDeque<DedupKey>,
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct DedupKey {
    pub chat_id: i64,
    pub token: i64,
}

impl UpdateDedup {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if the key is new (the caller should proceed) and
    /// `false` if it has already been observed recently (the caller should
    /// drop the update as a duplicate).
    pub async fn observe(&self, key: DedupKey) -> bool {
        let mut state = self.inner.lock().await;
        if state.seen.contains_key(&key) {
            counter!("gateway_update_duplicate_total").increment(1);
            return false;
        }
        if state.order.len() >= UPDATE_DEDUP_CAPACITY {
            if let Some(oldest) = state.order.pop_front() {
                state.seen.remove(&oldest);
            }
        }
        state.seen.insert(key, ());
        state.order.push_back(key);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn given_two_updates_same_chat_when_gate_acquired_then_second_waits() {
        let serializer = ChatSerializer::new();
        let first = serializer.acquire(42).await;
        let inner = serializer.clone();
        let waiter = tokio::spawn(async move { inner.acquire(42).await });

        // Yield enough times to let the waiter park on the mutex.
        for _ in 0..10 {
            tokio::task::yield_now().await;
        }
        assert!(
            !waiter.is_finished(),
            "waiter must not proceed before first guard drops"
        );

        drop(first);
        let _second = waiter.await.expect("waiter should finish after release");
    }

    #[tokio::test]
    async fn given_two_chats_when_gate_acquired_then_both_proceed_in_parallel() {
        let serializer = ChatSerializer::new();
        let _first = serializer.acquire(1).await;
        // Acquiring a *different* chat must not block.
        let _second = serializer.acquire(2).await;
    }

    #[test]
    fn given_fresh_barrier_when_try_consume_then_only_first_call_succeeds() {
        let barrier = UxBarrier::new();
        assert!(barrier.try_consume());
        assert!(!barrier.try_consume());
        assert!(barrier.is_spent());
    }

    #[tokio::test]
    async fn given_repeated_key_when_dedup_observed_then_second_is_dropped() {
        let dedup = UpdateDedup::new();
        let key = DedupKey {
            chat_id: 10,
            token: 7,
        };
        assert!(dedup.observe(key).await);
        assert!(!dedup.observe(key).await);
    }

    #[tokio::test]
    async fn given_onboarding_completion_then_second_handler_on_same_update_is_no_op() {
        // GIVEN: one update handed to the gateway, barrier cloned to both the
        //        "onboarding" and the "task-creation" consumers (mimicking the
        //        register_actor ↦ dispatch_message_inner split).
        let serializer = ChatSerializer::new();
        let guard = serializer.acquire(777).await;
        let onboarding_view = guard.barrier();
        let create_task_view = guard.barrier();

        // WHEN: onboarding renders the welcome screen first.
        assert!(
            onboarding_view.try_consume(),
            "onboarding must be allowed to render once"
        );

        // THEN: the orphan "create task" handler that historically produced
        //       the "Некорректный запрос" second reply must now be blocked.
        assert!(
            !create_task_view.try_consume(),
            "regression: second outbound UX effect on the same update was accepted"
        );
        assert!(onboarding_view.is_spent());
        assert!(create_task_view.is_spent());
    }

    #[tokio::test]
    async fn given_two_independent_updates_then_each_has_its_own_barrier() {
        let serializer = ChatSerializer::new();

        // First update on chat 1 consumes its barrier.
        {
            let guard = serializer.acquire(1).await;
            assert!(guard.barrier().try_consume());
            // Guard drops here, releasing the chat lock.
        }

        // Second update, also on chat 1 — fresh barrier, must succeed again.
        {
            let guard = serializer.acquire(1).await;
            assert!(
                guard.barrier().try_consume(),
                "new update must start with an unspent barrier"
            );
        }
    }
}
