# Telegram Task Bot

Production-oriented Telegram bot for task intake, assignment, review, blockers, comments, reminders, and manager visibility.

## What is implemented

- Clean architecture with `presentation -> application -> domain <- infrastructure`
- Telegram text and voice intake
- Quick task creation and guided 3-step wizard
- Assignee resolution by `@username` or employee directory with ambiguity handling
- SQLite persistence with migrations, optimistic locking, audit log, comments, and notifications queue
- Separate task business state and notification delivery state
- Background jobs for employee sync, notification delivery, deadline reminders, overdue alerts, and daily summaries
- Manager/admin views for team tasks and team stats
- Health and metrics endpoints

## Main product flows

- `/start` opens a clear main menu with personal and manager sections
- Quick create is best when the task is already written in one message or voice note
- Guided create is best when you want to avoid missing assignee, scope, or deadline
- Task cards support status changes, review flow, blockers, comments, and reassignment
- Dangerous actions such as cancel require explicit confirmation
- If the assignee is found but has not started the bot, the task is still created and the card shows that delivery is waiting for `/start`

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
- `/status <task_uid>`
- `/cancel_task <task_uid>`
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
- [docs/quality-roadmap.md](./docs/quality-roadmap.md)

## Docker

- `docker compose up telegram-task-bot`
- `docker compose run --rm tests`

The Docker setup is tuned to avoid the Windows host issues we saw earlier with SQLite bind mounts and high parallel Rust builds.

## Local verification note

In this Windows workspace `cargo check` is currently blocked by local application policy because the installed Rust toolchain is `x86_64-pc-windows-gnu` and the system refuses `gcc.exe`. Docker was not run in this iteration by request, so final end-to-end runtime verification should be done either:

- in Docker, or
- after switching the local Rust toolchain to an allowed MSVC target
