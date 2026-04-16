# Telegram Task Bot

Production-oriented Telegram bot for task intake, assignment, review, blockers, comments, reminders, and manager visibility.

## What is implemented

- Clean architecture with `presentation -> application -> domain <- infrastructure`
- Telegram text and voice intake
- Quick task creation and guided 3-step wizard
- Voice task creation with mandatory confirmation before the task is created
- Assignee resolution by `@username` or employee directory with ambiguity handling
- SQLite persistence with migrations, optimistic locking, audit log, comments, and notifications queue
- Separate task business state and notification delivery state
- Background jobs for employee sync, notification delivery, deadline reminders, overdue alerts, and daily summaries
- Manager/admin views for team tasks and team stats
- Public task codes like `T-0042` instead of UUID-only user references
- Compact and expanded task cards with highlighted next action
- Personal `Мой фокус` screen and manager `Inbox менеджера`
- Edit-in-place screen navigation for menu, lists, task cards, confirmations, and wizard flows
- Health and metrics endpoints

## Main product flows

- `/start` opens a clear main menu with personal and manager sections
- `/start` exposes `Мой фокус` as the main daily-use entry point
- Quick create is best when the task is already written in one message or voice note
- Guided create is best when you want to avoid missing assignee, scope, or deadline
- Task cards open in compact mode first and can expand into full details
- Task cards support status changes, review flow, blockers, comments, and reassignment
- Dangerous actions such as cancel require explicit confirmation
- If the assignee is found but has not started the bot, the task is still created and the card shows that delivery is waiting for `/start`
- The task card explains what to do when the assignee has not started the bot yet and offers a dedicated help screen for that case
- If the assignee starts the bot later, open employee-assigned tasks are linked automatically and assignment delivery is backfilled safely
- Duplicate detection opens the existing task instead of pretending a new one was created
- Navigation screens are edited in place, while real events such as assignment notifications and reminders remain separate messages

## Task lifecycle

- `created`
- `sent`
- `in_progress`
- `blocked`
- `in_review`
- `completed`
- `cancelled`

Status transitions are enforced by a single state machine in the domain layer. Delivery states are tracked separately: `pending`, `sent`, `retry_pending`, `failed`.

## Important Telegram limitation

Telegram bots cannot initiate a private chat by `@username` alone. The bot stores `last_chat_id` only after the user sends `/start` or any message. Because of that, a task may be assigned correctly while delivery remains in `pending registration`.

## Environment setup

1. Copy `.env.example` to `.env`
2. Fill required values:
   - `TELEGRAM_BOT_TOKEN`
   - `DATABASE_URL`
   - `GOOGLE_GEMINI_API_KEY`
   - `OPENAI_API_KEY`
   - `GOOGLE_SHEETS_ID`
   - one of `GOOGLE_SHEETS_API_KEY` or `GOOGLE_SHEETS_BEARER_TOKEN`
3. Optional scheduler settings:
   - `DEADLINE_REMINDER_HOUR_UTC`
   - `OVERDUE_SCAN_HOUR_UTC`
   - `DAILY_SUMMARY_HOUR_UTC`

## Main commands

- `/start`
- `/help`
- `/new_task <text>`
- `/my_tasks [cursor]`
- `/created_tasks [cursor]`
- `/team_tasks [cursor]`
- `/status <T-0001|task_uid>`
- `/cancel_task <T-0001|task_uid>`
- `/stats`
- `/team_stats`
- `/settings`
- `/admin_sync_employees`

## HTTP endpoints

- `GET /healthz`
- `GET /metrics`

## Documentation

- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [DEPLOYMENT.md](./DEPLOYMENT.md)
- [docs/memory.md](./docs/memory.md)
- [docs/system-model.md](./docs/system-model.md)
- [docs/telegram-ux.md](./docs/telegram-ux.md)
- [docs/operations.md](./docs/operations.md)
- [docs/quality-roadmap.md](./docs/quality-roadmap.md)

## Docker

- `docker compose up telegram-task-bot`
- `docker compose --profile test run --rm tests`
- `docker compose --profile smoke run --rm smoke-check`

The Docker setup now uses:
- multi-stage build
- separate test-runner stage
- slim runtime image
- non-root app execution through `gosu`
- healthcheck-ready compose wiring
- named volume persistence for SQLite

## Local verification note

In this Windows workspace:

- `cargo fmt --all` passes
- `cargo check` passes
- `cargo clippy --all-targets --all-features -- -D warnings` passes
- targeted `cargo test` execution passes

For final deploy confidence, keep Docker-based validation in the loop because it removes host-specific Windows variance from the verification path.
