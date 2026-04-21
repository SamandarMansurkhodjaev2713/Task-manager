# Operations

This document describes the compact operational model for the Telegram task bot.

## SQLite backup and restore

Recommended rule:
- always create a fresh SQLite snapshot before deploy
- keep at least one known-good rollback snapshot
- never run deploy and manual DB editing in parallel

Minimal backup checklist:
1. stop or drain write-heavy traffic if possible
2. copy the SQLite file from the persistent volume
3. record:
   - deploy timestamp
   - git revision
   - image tag
   - migration version
4. verify the backup file is readable

Minimal restore checklist:
1. stop the running application
2. restore the backup SQLite file into the persistent volume
3. redeploy the previous known-good image
4. verify:
   - `/healthz`
   - `/metrics`
   - queue is processing
   - main Telegram flows still work

## Deploy checks

Before deploy:
- `cargo fmt --all`
- `cargo check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- Docker test profile

After deploy:
- open `/healthz`
- open `/metrics`
- create one quick task
- create one guided task
- send one voice task and verify confirmation appears before creation
- assign one task to a user who already started the bot
- assign one task to a directory employee who has not started the bot yet
- verify the pending-registration card shows the correct delivery explanation
- verify late `/start` recovers delivery automatically

## Queue and delivery health

Watch for:
- growing retry queue
- repeated failed assignment notifications
- tasks stuck in `pending registration` longer than expected
- reminder backlog not draining

Healthy state:
- new assignment notifications either reach `sent` or have a clear, explainable pending reason
- retry queue does not grow monotonically
- no silent delivery failures

## Restart recovery expectations

After restart:
- active Telegram screens may need safe fallback recreation
- stale callback protection must still block unsafe mutations
- notification delivery must resume from persisted state
- tasks assigned before assignee registration must still recover after the assignee sends `/start`

## Operational invariants

- business task state must stay separate from notification delivery state
- the bot must never claim delivery success without evidence
- the bot must never claim a task was created if it was only deduplicated
- voice tasks must require confirmation before creation
- navigation screens may be recreated safely, but event messages must stay in history

## Observability endpoints

The application exposes a small, stable HTTP surface for probes and
scraping.  These endpoints intentionally stay independent of the Telegram
dispatcher so they remain queryable even while Telegram traffic is
throttled.

| Endpoint        | Purpose                                        | Expected probe           |
| --------------- | ---------------------------------------------- | ------------------------ |
| `/healthz`      | Process liveness (no external calls)           | 200 always when alive    |
| `/healthz/deep` | Readiness: verifies the SQLite pool is usable  | 200 ok / 503 degraded    |
| `/metrics`      | Prometheus text format (`version=0.0.4`)       | scraped by Prometheus    |
| `/version`      | Build metadata (name, version, git sha, rustc) | cross-checked in deploys |

Semantics:
- `/healthz` must never touch the DB so orchestrators cannot flap on
  transient storage hiccups.
- `/healthz/deep` runs a bounded `SELECT 1` against SQLite and reports
  latency in milliseconds; a failing pool returns HTTP 503 with structured
  diagnostics.
- `/version` returns JSON with `git_sha` populated from the `GIT_SHA`
  build-time environment variable.  If not injected at build time the
  field is reported as `"unknown"` — deploys should set it via Docker
  `--build-arg GIT_SHA=$(git rev-parse HEAD)`.

## Log inventory

The application emits structured, JSON-formatted logs via `tracing`.  All
records include an `event` (logical name), `level`, `timestamp` and a
`trace_id` when available.  The stable event families are:

| Event family               | Level         | Where emitted                             | Purpose                                              |
| -------------------------- | ------------- | ----------------------------------------- | ---------------------------------------------------- |
| `bootstrap.*`              | info / warn   | `run_application`, `BootstrapAdminsUseCase` | Startup wiring & initial admin elevation             |
| `http.request`             | info / error  | `TraceLayer` in `presentation::http`       | Per-request access log for `/healthz`, `/metrics`…   |
| `telegram.update`          | info / warn   | `dispatcher::run_telegram_dispatcher`     | Telegram update ingress + unhandled-update warnings  |
| `telegram.callback`        | info          | `dispatcher_handlers`                     | Inline keyboard callbacks with sanitised payloads    |
| `usecase.<name>.{ok,err}`  | info / error  | Each use case via `tracing::instrument`   | Business outcome + duration histogram                |
| `scheduler.job.*`          | info / error  | `infrastructure::scheduler::BackgroundJobs` | Cron/interval job starts, outcomes, retries          |
| `notification.delivery.*`  | info / warn   | `process_notifications`, `bot_gateway`    | Outbound notification attempts and pending reasons   |
| `security.audit`           | warn / error  | `AdminUseCase`, RBAC policy rejections     | Role changes, deactivations, forbidden admin calls   |
| `profile.analytics`        | warn          | `show_settings`                           | Logged only when personal stats fetch fails          |

All PII-bearing fields (full names, usernames, Telegram IDs, message
bodies) MUST pass through the `PIIRedactor` before being logged.  The
redactor is installed globally during bootstrap and uses SHA-256 with a
per-process salt in production; tests pin a nil-salt variant so
assertions are deterministic.
