//! Integration-level mirror of the unit tests in
//! `src/presentation/telegram/gateway.rs`.  The project's local Windows
//! environment blocks the main `lib.rs` test binary from running (AppControl
//! policy), so we duplicate the crucial invariants here where the integration
//! test binary is allowed to execute.  Logic is intentionally identical so
//! that CI and local runs exercise the same guarantees.

use telegram_task_bot::presentation::telegram::gateway::{
    ChatSerializer, DedupKey, UpdateDedup, UxBarrier,
};

#[tokio::test]
async fn given_two_updates_same_chat_when_gate_acquired_then_second_waits() {
    let serializer = ChatSerializer::new();
    let first = serializer.acquire(42).await;
    let waiter = {
        let serializer = serializer.clone();
        tokio::spawn(async move { serializer.acquire(42).await })
    };

    for _ in 0..16 {
        tokio::task::yield_now().await;
    }
    assert!(
        !waiter.is_finished(),
        "second update must not proceed while the first guard is live"
    );

    drop(first);
    let _second = waiter.await.expect("waiter must complete after release");
}

#[tokio::test]
async fn given_two_chats_when_gate_acquired_then_both_proceed_in_parallel() {
    let serializer = ChatSerializer::new();
    let _first = serializer.acquire(1).await;
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
    // GIVEN one update and two handlers sharing the same barrier clone,
    //       mirroring the register_actor ↦ dispatch_message_inner split.
    let serializer = ChatSerializer::new();
    let guard = serializer.acquire(777).await;
    let onboarding_view = guard.barrier();
    let create_task_view = guard.barrier();

    // WHEN onboarding renders first.
    assert!(onboarding_view.try_consume());

    // THEN the orphan "create task" handler must be blocked — this is the
    //      regression that used to surface as "Welcome" + "Некорректный
    //      запрос" on the same Telegram update.
    assert!(!create_task_view.try_consume());
    assert!(onboarding_view.is_spent());
    assert!(create_task_view.is_spent());
}

#[tokio::test]
async fn given_two_independent_updates_then_each_has_its_own_barrier() {
    let serializer = ChatSerializer::new();

    {
        let guard = serializer.acquire(1).await;
        assert!(guard.barrier().try_consume());
    }

    {
        let guard = serializer.acquire(1).await;
        assert!(
            guard.barrier().try_consume(),
            "a fresh update must start with an unspent barrier"
        );
    }
}
