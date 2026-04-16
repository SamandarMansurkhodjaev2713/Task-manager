# Deployment

## Recommended runtime stack

- Linux host or container runtime
- Rust stable runtime image
- SQLite on a persistent volume
- outbound HTTPS access to:
  - `api.telegram.org`
  - `generativelanguage.googleapis.com`
  - `sheets.googleapis.com`
  - `api.openai.com`

## Boot checklist

1. Fill `.env` from `.env.example`
2. Provide all required secrets through environment variables
3. Mount persistent storage for SQLite
4. Run migrations on startup
5. Ask employees to send `/start` to the bot at least once
6. Verify:
   - `GET /healthz`
   - `GET /metrics`
7. Check that the notification queue drains successfully

## Important Telegram constraint

The bot cannot proactively DM a user by `@username` alone.
Direct delivery requires a known `chat_id`, which becomes available only after the user has interacted with the bot.

Because of that:
- assignment can succeed even if direct delivery is still pending
- task cards must show delivery status honestly

## Suggested local commands

```powershell
cargo fmt --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
cargo run
```

## Current local verification note

In the current Windows workspace:
- `cargo fmt --check` works
- `cargo check` works
- `cargo clippy --all-targets --all-features -- -D warnings` works
- targeted `cargo test` runs work

For deployment sign-off, keep Docker-based validation as the last gate to remove host-specific variance.

## Docker

Primary commands:

```powershell
docker compose up telegram-task-bot
docker compose --profile test run --rm tests
docker compose --profile smoke run --rm smoke-check
```

Current Docker setup uses:
- multi-stage build
- dedicated runtime image
- dedicated test-runner image stage
- non-root app execution through `gosu`
- healthcheck on `/healthz`
- a named volume for SQLite persistence
- explicit `DATABASE_URL` override inside the container
- read-only runtime filesystem outside writable mounts
- dedicated `/tmp` tmpfs for transient runtime files
- graceful stop window through `stop_grace_period`
- isolated test profile with dummy secrets instead of the live `.env`

## Migration strategy

- migrations must remain additive
- snapshot the SQLite database before rollout
- if rollback is needed, restore the snapshot and redeploy the previous binary

## Operational checks after deployment

- create task from quick mode
- create task from guided mode
- create task from voice mode and verify confirmation appears before creation
- assign task to a user who already started the bot
- assign task to a directory employee who has not started the bot
- verify that the same employee receives linked tasks automatically after their first `/start`
- verify that the pending-registration card shows the dedicated help screen
- verify comment / blocker / review / reassignment flows
- verify overdue and reminder jobs
- verify queue retry behavior

## Documentation map

- [README.md](./README.md)
- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [docs/memory.md](./docs/memory.md)
- [docs/operations.md](./docs/operations.md)
- [docs/quality-roadmap.md](./docs/quality-roadmap.md)
