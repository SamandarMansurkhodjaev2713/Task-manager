# Project Memory

This file is the working memory for future iterations on the Telegram task bot.
It should let an engineer resume work without re-reading the whole codebase.

## Snapshot

- Project: Telegram task bot for team task management
- Stack: Rust, Teloxide, SQLite, clean architecture, background jobs
- Current focus: move from "strong base" to genuinely production-grade quality
- Local environment constraint:
  - `cargo fmt --all` works
  - local `cargo check` now works in this workspace
  - local `cargo test` succeeded in a full run once during this iteration
  - repeated execution of some compiled test binaries may still be blocked by Windows application control policy (`os error 4551`)
  - Docker validation was intentionally avoided in this implementation round unless explicitly requested

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

### 8. Public task code is derived from persisted task id

Chosen:
- user-facing task reference is `T-####`
- internal identity remains UUID v7
- `/status` and `/cancel_task` now accept public code or UUID

Why:
- users should not work with raw UUIDs
- task ids already exist, are stable, and do not require a risky schema migration
- this keeps backward compatibility while dramatically improving UX

Rejected:
- adding a second stored public id sequence in the database

Reason rejected:
- unnecessary migration risk for this stage
- existing `tasks.id` already provides a safe monotonic reference

### 9. Truthful duplicate flow is explicit

Chosen:
- `TaskCreationOutcome` now distinguishes:
  - `Created`
  - `DuplicateFound`
  - `ClarificationRequired`

Why:
- duplicate detection should never be rendered as successful creation
- this directly fixes a trust-breaking UX bug

Rejected:
- reusing `Created` with a different message body

Reason rejected:
- presentation layer cannot reliably distinguish a real creation from a dedupe hit
- leads to confusing copy and wrong affordances

### 10. Task card now has compact and expanded modes

Chosen:
- default open is compact
- expanded mode is opt-in via callback
- compact card highlights:
  - public code
  - status
  - deadline
  - assignee
  - delivery
  - next best action

Why:
- the old card overloaded one message with too much detail
- compact first, details on demand is a better Telegram pattern

Rejected:
- always-expanded task card

Reason rejected:
- too much scrolling
- higher cognitive load on mobile

### 11. Focus and manager inbox are first-class task list modes

Chosen:
- `Focus` for individual daily work
- `ManagerInbox` for review/blocker/risk monitoring

Why:
- plain task lists are not enough for day-to-day prioritization
- managers need a “what needs my decision” screen, not just a raw team backlog

Rejected:
- keeping only `Assigned`, `Created`, and `Team`

Reason rejected:
- weak prioritization UX
- too much manual scanning

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

## Current user-facing navigation

- Main menu:
  - `Мой фокус`
  - `Создать задачу`
  - `Мои задачи`
  - `Созданные мной`
  - manager-only:
    - `Командные задачи`
    - `Inbox менеджера`
- Task card:
  - compact by default
  - expanded on demand
  - primary action highlighted first
  - dangerous action isolated
- Commands:
  - user-facing task reference is short code like `T-0042`

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
- `src/presentation/telegram/ui_keyboards.rs`

### 2. Local build verification is incomplete

- formatting is verified
- compile is verified locally via `cargo check`
- tests are verified from a full successful run in this workspace
- one environment-specific risk remains:
  - some repeated test executable launches may be blocked by Windows application control policy

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
2. Add real button-based ambiguity resolution and assignee suggestions
3. Strengthen tests around stale callbacks/conflicts/retries
4. Expand blocker escalation and manager review ergonomics
5. Add stronger operational docs and runbooks

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
