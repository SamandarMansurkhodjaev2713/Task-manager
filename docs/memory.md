# Project Memory

This file is the working memory for future iterations on the Telegram task bot.
It should let an engineer resume work without re-reading the whole codebase.

## Snapshot

- Project: Telegram task bot for team task management
- Stack: Rust, Teloxide, SQLite, clean architecture, background jobs
- Current focus: production-grade UX, reliable state transitions, low-chat-noise navigation
- Local environment constraint:
- `cargo fmt --all` works
- `cargo check` works
- `cargo clippy --all-targets --all-features -- -D warnings` works
- targeted `cargo test` execution works in this workspace
- some broader test executions may still remain sensitive to local Windows policy, so Docker validation stays part of the final gate

## Chosen architectural model

- Layer rule:
  - `presentation -> application -> domain <- infrastructure`
- Domain is pure:
  - no Telegram imports
  - no DB imports
  - no reqwest/teloxide/sqlx in domain
- Application layer owns orchestration:
  - authorization checks
  - sequencing of repositories/services
  - notification enqueue
  - audit append
- Infrastructure owns side effects:
  - SQLite repositories
  - Telegram delivery
  - AI providers
  - scheduler

## Key decisions already made

### 1. Task business state is separate from notification delivery state

Chosen:
- task lifecycle is stored on `Task`
- delivery lifecycle is stored on `Notification`

Rejected:
- overloading task status with delivery semantics

Why:
- business progress and Telegram delivery are different truths
- this avoids lying in the UI when delivery is pending or failed

### 2. Guided flow and quick flow share one creation pipeline

Chosen:
- guided flow builds a synthetic `IncomingMessage`
- both flows go through `CreateTaskFromMessageUseCase`
- guided idempotency uses `source_message_key_override`

Rejected:
- separate guided-only create pipeline

Why:
- avoids drift between quick and guided behavior
- keeps parser, assignee resolution, dedupe, and task generation consistent

### 3. Assignee resolution is centralized

Chosen:
- `AssigneeResolver` handles:
  - username lookup
  - employee directory matching
  - ambiguity outcome
  - “user has not started bot” scenarios

Rejected:
- partial matching directly in handlers or individual use cases

Why:
- one source of truth for assignment semantics

### 4. Optimistic locking on tasks

Chosen:
- `tasks.version`
- repository update checks version

Rejected:
- last write wins

Why:
- protects from double updates and concurrent manager/executor actions

### 5. Reassignment resets work-specific lifecycle

Chosen:
- reassignment resets the task back to a clean assignment state
- work/review/blocker progress is not silently inherited by a new assignee

Rejected:
- keeping `sent` or `in_progress` after reassignment

Why:
- semantically wrong
- delivery and accountability become misleading

### 6. Review flow is explicit

Chosen:
- assignee moves work to `in_review`
- creator/manager/admin can approve completion
- creator/manager can return work back

Rejected:
- direct completion for every executor

Why:
- better control model for real team workflows

### 7. Public task code is derived from persisted task id

Chosen:
- user-facing task reference is `T-####`
- internal identity remains UUID v7
- commands accept public code or UUID

Rejected:
- introducing a second stored public sequence immediately

Why:
- existing `tasks.id` already provides a stable monotonic reference
- avoids risky migration for little gain

### 8. Truthful duplicate flow is explicit

Chosen:
- `TaskCreationOutcome` distinguishes:
  - `Created`
  - `DuplicateFound`
  - `ClarificationRequired`

Rejected:
- reusing “created” with different copy

Why:
- UI must never claim success when a task was not created

### 9. Task card has compact and expanded modes

Chosen:
- compact is default
- expanded is opt-in

Rejected:
- always-expanded task card

Why:
- compact first is a better mobile Telegram pattern

### 10. Focus and manager inbox are first-class views

Chosen:
- `Focus` for individual prioritization
- `ManagerInbox` for decision-heavy managerial work

Rejected:
- only raw assigned/created/team lists

Why:
- day-to-day work needs prioritization, not only storage

### 11. Navigation now uses edit-in-place screens

Chosen:
- menu, lists, task cards, confirmations, wizard steps, and text prompts edit one active screen message per chat
- event notifications remain separate history messages
- a central transport decides edit vs safe fallback send

Rejected:
- naïve “replace some send_message with edit_message_text”
- deleting old messages aggressively
- converting all notifications into edits

Why:
- chat should feel like a mini-app, not a bot log
- deleting old content is brittle and can destroy useful history
- true events must remain visible in the chat timeline

### 12. Late assignee registration now recovers existing assignments

Chosen:
- when an employee starts the bot after tasks were assigned only through the employee directory,
  registration reconciles open tasks from `assigned_to_employee_id` to `assigned_to_user_id`
- assignment notifications are enqueued or re-queued after the user becomes reachable
- the recovery is idempotent and conflict-aware

Rejected:
- leaving recovery to manual reassignment
- silently keeping tasks visible only in manager views until someone edits them

Why:
- this was a real product bug
- assignment should become actionable automatically after `/start`
- recovery must be safe on repeated registration and concurrent updates

### 13. SQLite repositories are now split per aggregate

Chosen:
- user, employee, task, notification, audit, and comment repositories live in separate files
- shared SQL mapping helpers live in a dedicated `common.rs`

Rejected:
- keeping one giant `repositories.rs`

Why:
- lower cognitive load
- safer review surface
- easier future refactors and testing

### 14. Docker runtime is now multi-stage and non-root for the app process

### 15. Voice creation now requires an explicit confirmation screen

Chosen:
- voice messages are transcribed first
- the bot shows one confirmation screen with the transcript
- the user can confirm, edit the text, or cancel
- task creation happens only after explicit confirmation

Rejected:
- direct task creation immediately after transcription

Why:
- speech recognition is helpful but not trustworthy enough to create tasks silently
- this keeps the fast Telegram voice workflow without sacrificing product trust

### 16. Pending-registration delivery now has a dedicated help path

Chosen:
- compact task cards explain that the task is already saved
- a dedicated help screen gives the author a ready-to-forward `/start` instruction for the assignee

Rejected:
- leaving the state as a bare badge only

Why:
- `waiting for /start` is technically correct but operationally incomplete
- the author needs one obvious next step, not only a status label

### 17. Assignee matching now prefers safety over convenience

Chosen:
- exact `@username` may auto-resolve
- exact full name may auto-resolve
- exact first name resolves only when it is unique
- typo-like or fuzzy full-name matches never auto-assign silently
- ambiguous names always go through explicit button-based choice

Rejected:
- silently assigning on the best fuzzy candidate
- treating a close full-name suggestion as safe enough

Why:
- the cost of a wrong assignee is higher than the cost of one extra clarification step
- small-team trust depends on the bot never sending work to the wrong person

### 18. Registration now has an explicit employee-linking model

Chosen:
- users can be stored unlinked
- once linked, `users.linked_employee_id` becomes the durable identity bridge
- `/start` may reopen linking clarification for still-unlinked users
- ambiguous employee linking is explicit and button-driven
- continuing without linking is allowed, but it is a deliberate choice

Rejected:
- implicit “probably this employee” auto-linking
- relying forever on display-name coincidence without a durable link

Why:
- registration must be safe, short, and reversible
- late assignment recovery needs a real employee link, not a lucky name match

Chosen:
- multi-stage Docker build
- dedicated runtime image
- app process starts through `gosu` as `taskbot`
- compose includes healthcheck and smoke profile

Rejected:
- single-stage builder/runtime image
- running the bot process as root forever

Why:
- smaller runtime surface
- better deploy hygiene
- safer persistent volume usage

## Current task lifecycle

- `created`
- `sent`
- `in_progress`
- `blocked`
- `in_review`
- `completed`
- `cancelled`

Important nuance:
- `sent` means the assignment notification was actually delivered
- `created` can still have an assignee if delivery has not happened yet

## Current screen lifecycle model

- One active screen is tracked per chat via `ActiveScreenStore`
- Each screen stores:
  - `message_id`
  - `ScreenDescriptor`
- Navigation rendering goes through `send_screen`
- `send_screen` behavior:
  - try edit current active screen
  - if Telegram says “message is not modified”, treat it as success
  - if screen is not editable or not found, send exactly one fallback screen
  - update active screen pointer after successful edit or fallback send
- Stale callback safety:
  - if callback comes from a non-active message and action is mutating, reject it safely
  - if callback comes from a non-active message and action is navigational, acknowledge and reopen fresh state

## What is intentionally still separate messages

- assignment notifications
- reminders
- daily digests
- overdue alerts
- blocker escalations
- reassignment notifications
- fatal error fallback when no editable screen is available

## Known limitations left on purpose

- stale safety is currently based on active message identity, not task-version-aware callback tokens
- assignee ambiguity still returns clarification text; it is not yet a fully button-driven candidate picker
- some modules are still too large and should be split further in future iterations
- some broader runtime test passes should still be repeated in Docker to eliminate host-specific variance completely

## Rejected alternatives

### Delete previous bot messages on every click

Rejected because:
- brittle under Telegram API limitations
- can fail noisily if the message is old or already gone
- removes useful history the user may want to keep

### Keep all screens as separate messages

Rejected because:
- causes chat spam
- creates stale buttons everywhere
- makes the bot feel like a script, not a product

### Make notifications edit the active screen too

Rejected because:
- real events should remain visible later in chat history
- users can miss assignments/reminders if everything mutates one screen

## Next strong follow-up targets

- version-aware stale callback protection
- button-based ambiguity resolution
- deeper decomposition of large presentation/domain files
- broader test harness around transport/edit fallback behavior

## 17. Runtime container is now hardened beyond basic non-root execution

Decision:
- the main Docker runtime service now runs with a read-only filesystem
- only the SQLite volume stays writable
- `/tmp` is exposed as tmpfs for transient runtime files
- `stop_grace_period` is explicitly set for calmer shutdown behavior

Why:
- reduces accidental filesystem writes outside the data volume
- makes runtime behavior closer to a production least-privilege model
- keeps SQLite persistence explicit instead of implicit

Rejected alternative:
- keep the container writable everywhere because it is simpler

Why rejected:
- easier to miss unintended writes
- weaker deploy posture for no real product benefit

## 18. Docker test profile must not depend on live secrets

Decision:
- the `tests` compose profile now uses dummy environment values instead of inheriting the live `.env`

Why:
- containerized test runs should be reproducible without production-like secrets
- this lowers the risk of accidentally coupling test execution to a real bot token or API keys
- test isolation is more important than mirroring the full runtime secret set

Rejected alternative:
- reuse `.env` everywhere for convenience

Why rejected:
- increases secret exposure risk
- makes CI and local test runs more fragile
- hides the difference between runtime config and test-only requirements

## 19. Production remediation pass — confirmed findings and fixes applied (2026-04-18)

A full production review was conducted against the codebase and resulted in a remediation
pass that addressed all confirmed Critical and High issues plus material Medium issues.

**Confirmed Critical — fixed:**

- **C-1** (`Dockerfile`): test-runner stage now runs `cargo test --lib --workspace` during
  `docker build`. `CARGO_BUILD_JOBS=1` removed from both stages (also fixes OPS-8).
- **C-2** (`migrations/005_employees_remove_full_name_unique.sql`,
  `employee_repository.rs`): removed `UNIQUE` on `employees.full_name`; replaced
  `ON CONFLICT(full_name)` upsert with a two-path strategy: match by
  `telegram_username` (with a partial unique index) when present, otherwise find-and-update
  or insert for no-username rows. Same-name employees with different Telegram accounts
  no longer clobber each other.
- **C-3** (`dispatcher_transport.rs`): `send_error()` no longer leaks internal English
  dev messages to end users. All variants now produce Russian user-facing strings. A
  `tracing::warn!` logs the internal code + message for ops.

**Confirmed High — fixed:**

- **H-1 + M-22** (`dispatcher_transport.rs`): `is_edit_fallback_error` and
  `is_message_not_modified_error` now match against `teloxide::RequestError::Api` only;
  network / IO errors are explicitly excluded from these patterns.
- **H-3** (`pool.rs`): SQLite WAL mode enabled (`journal_mode = WAL`,
  `synchronous = NORMAL`). Concurrent readers no longer block writers.
- **H-4** (`process_notifications.rs`): notification delivery is now concurrent
  (up to `MAX_CONCURRENT_NOTIFICATION_DELIVERIES = 5` in flight at once) via
  `tokio::task::JoinSet` + `Semaphore`. Batch errors are collected rather than
  short-circuiting.
- **H-6** (`DEPLOYMENT.md`): concrete snapshot-based rollback procedure with shell
  scripts documented.
- **H-10** (`bot_gateway.rs`, `process_notifications.rs`): `TeloxideNotifier` now emits
  `TELEGRAM_BOT_BLOCKED` / `TELEGRAM_CHAT_NOT_FOUND` for permanent delivery failures.
  `ProcessNotificationsUseCase` checks these codes and calls `mark_failed` immediately
  instead of exhausting retry attempts.

**Confirmed Medium — fixed:**

- **M-1** (`role_authorization.rs`): removed redundant `|| is_admin` from the Cancel
  action check (covered by `can_manage` which already includes admins).
- **M-7** (`dispatcher_assignee_clarifications.rs`): `clarification_candidate_ids` is now
  the single canonical copy (`pub(crate)`); removed duplicates from `dispatcher_guided.rs`
  and `dispatcher_voice.rs`.
- **M-23** (`process_notifications.rs`, `reliability.rs`): exponential back-off for
  notification retries (`base 60 s × 2^(attempt-1)`, capped at 3600 s) replacing the
  previous fixed 60 s delay.

**Confirmed Medium — not applied (design decision):**

- **M-4** (`GuidedTaskDraft.submission_key`): reverted — the field IS read by
  `build_guided_message` to create the deduplication `source_message_key` for guided
  submissions. Finding was a false positive.

**Confirmed Outdated:**

- **OPS-4 (healthcheck)**: `/healthz` was already implemented in `src/presentation/http/mod.rs`.

**New integration tests added:**

- `tests/employee_repository_integration.rs` — 4 tests covering same-name upsert,
  username-based upsert idempotency, and no-username upsert idempotency.
- `tests/notification_processing_tests.rs` — 3 tests covering permanent failure
  fast-path (bot blocked), transient retry scheduling, and last-attempt permanent fail.

**Known remaining risks (deferred):**

- H-2 / H-8: `count_stats_global` is an unbounded full-table scan (no caching).
  Acceptable for ≤ 40 users; add a 30-second in-memory TTL cache before scaling.
- Circuit-breaker state is in-process; resets on restart. Non-critical for single-instance.
- SQLite WAL journal file is not explicitly checkpointed; may grow under high write load.
  Monitor `wal_autocheckpoint` pragma if needed.
