# Project Memory

This file is the working memory for future iterations on the Telegram task bot.
It should let an engineer resume work without re-reading the whole codebase.

## Snapshot

- Project: Telegram task bot for team task management
- Stack: Rust, Teloxide, SQLite, clean architecture, background jobs
- Current focus: move from "strong base" to genuinely production-grade quality
- Local environment constraint:
  - `cargo fmt --check` works
  - local `cargo check` is blocked by Windows policy because the installed target is `x86_64-pc-windows-gnu` and `gcc.exe` is disallowed
  - Docker validation was intentionally avoided in the last implementation round unless explicitly requested

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

Why:
- "task assigned" and "notification delivered" are not the same event
- this avoids lying in the UI when Telegram delivery is pending or failed

Rejected:
- storing delivery state by overloading task status

Reason rejected:
- mixes business semantics with transport semantics
- makes reassignment and retries incorrect

### 2. Guided flow and quick flow must share one creation pipeline

Chosen:
- guided flow builds a synthetic `IncomingMessage`
- both flows go through `CreateTaskFromMessageUseCase`
- guided idempotency uses `source_message_key_override`

Why:
- avoids duplicated creation logic
- keeps parser/assignee resolution/task generation consistent

Rejected:
- separate guided-only create pipeline

Reason rejected:
- too much duplication
- higher chance of drift between quick and guided behavior

### 3. Assignee resolution is centralized

Chosen:
- `AssigneeResolver` handles:
  - username lookup
  - employee directory matching
  - ambiguity flow
  - "user has not started bot" scenarios

Why:
- single source of truth for assignment semantics

Rejected:
- partial assignee matching directly in handlers or multiple use cases

Reason rejected:
- causes inconsistent assignment behavior and fragmented UX

### 4. Optimistic locking on tasks

Chosen:
- `tasks.version`
- repository update checks version

Why:
- protects from double updates and race conditions
- especially important for Telegram callbacks and concurrent managers

Rejected:
- last write wins

Reason rejected:
- unsafe for task status changes, reassignment and review flow

### 5. Reassignment resets work-specific lifecycle

Chosen:
- reassignment moves task back to `created`
- progress/review/blocker timestamps are cleared
- delivery is re-driven through notification pipeline

Why:
- new assignee should not inherit stale "in progress" or "in review" state

Rejected:
- keeping `sent` or `in_progress` on reassignment

Reason rejected:
- semantically wrong
- delivery status becomes misleading

### 6. Review flow is explicit

Chosen:
- assignee can move work to `in_review`
- creator/manager/admin can approve completion
- creator/manager can return task to work

Why:
- matches real team workflows better than direct assignee completion

Rejected:
- direct `completed` for all assignees

Reason rejected:
- weak control model
- especially wrong when executor is not task author

### 7. Notification failures are not silent

Chosen:
- queue states:
  - `pending`
  - `sent`
  - `retry_pending`
  - `failed`
- retries are explicit
- task card can show delivery visibility

Rejected:
- mark failed immediately without state visibility
- or pretend delivery was successful after enqueue

Reason rejected:
- causes false success signals
- breaks trust in bot

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

## Current delivery lifecycle

- `pending`
- `sent`
- `retry_pending`
- `failed`

Important nuance:
- "assignee exists but never started the bot" is a distinct UX condition
- it is not equivalent to Telegram delivery failure

## Important files to know first

- Domain:
  - `src/domain/task.rs`
  - `src/domain/notification.rs`
  - `src/domain/comment.rs`
  - `src/domain/parsing.rs`
- Application:
  - `src/application/use_cases/create_task_from_message.rs`
  - `src/application/use_cases/update_task_status.rs`
  - `src/application/use_cases/get_task_status.rs`
  - `src/application/use_cases/assignee_resolution.rs`
  - `src/application/use_cases/reassign_task.rs`
  - `src/application/use_cases/report_task_blocker.rs`
  - `src/application/use_cases/add_task_comment.rs`
  - `src/application/use_cases/process_notifications.rs`
- Presentation:
  - `src/presentation/telegram/dispatcher.rs`
  - `src/presentation/telegram/dispatcher_handlers.rs`
  - `src/presentation/telegram/dispatcher_guided.rs`
  - `src/presentation/telegram/dispatcher_interactions.rs`
  - `src/presentation/telegram/callbacks.rs`
  - `src/presentation/telegram/ui_text.rs`
  - `src/presentation/telegram/ui_keyboards.rs`
- Infrastructure:
  - `src/infrastructure/db/repositories.rs`
  - `src/infrastructure/db/models.rs`
  - `src/infrastructure/telegram/bot_gateway.rs`
  - `src/infrastructure/scheduler/mod.rs`

## Known unresolved weaknesses

These are still real issues and should not be forgotten.

### 1. Several files are still too large

Highest-priority decomposition targets:
- `src/infrastructure/db/repositories.rs`
- `src/presentation/telegram/dispatcher_guided.rs`
- `src/presentation/telegram/dispatcher_handlers.rs`
- `src/domain/task.rs`
- `src/presentation/telegram/callbacks.rs`
- `src/presentation/telegram/ui_text.rs`

### 2. Local build verification is incomplete

- formatting is verified
- compile/test verification is incomplete locally because of the blocked GNU toolchain
- must validate either:
  - in Docker, or
  - with an MSVC Rust toolchain

### 3. Test matrix is still not strong enough for true 200/200

Need more tests for:
- stale callback behavior
- repository conflict paths
- reassignment matrix
- delivery retry behavior
- scheduler behavior
- manager/creator/assignee permission matrix

### 4. Documentation still needs deeper operational runbooks

Useful next docs:
- incident runbook
- callback compatibility notes
- migration/rollback checklist
- release checklist

## Things explicitly not chosen

### 1. No fake "delivery success" immediately after enqueue

Not chosen because it would mislead the author.

### 2. No assignment by username alone without known chat history

Not chosen because Telegram bot delivery requires `chat_id`, not just `@username`.

### 3. No separate business logic in Telegram handlers

Not chosen because it would fragment rules and make testing much harder.

### 4. No silent fallback when assignee match is ambiguous

Not chosen because wrong assignment is worse than asking for clarification.

### 5. No direct completion for assignee when review is required

Not chosen because it weakens managerial control and breaks review semantics.

## Recommended next iteration order

1. Decompose the largest remaining files
2. Expand tests around conflicts/stale callbacks/retries
3. Add stronger operational docs and runbooks
4. Validate full build/test flow in a supported environment
5. Do one final consistency pass on Telegram UX text and empty/error states

## How to use this file

Before re-analyzing the codebase:
- read this file first
- read `README.md`
- read `ARCHITECTURE.md`
- then inspect only the specific modules related to the current task

If a future change invalidates any decision here:
- update this file immediately
- note what changed
- note why the previous decision stopped being valid
