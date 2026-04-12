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

## Current local environment caveat

In the current Windows workspace:
- `cargo fmt --check` works
- local `cargo check` is blocked because the installed Rust target is `x86_64-pc-windows-gnu`
- Windows policy blocks `gcc.exe`, which is needed by transitive native build steps

Recommended fixes:
- switch to an MSVC Rust toolchain for local verification, or
- validate build and tests through Docker

## Docker

Primary commands:

```powershell
docker compose up telegram-task-bot
docker compose run --rm tests
```

Current Docker setup uses:
- low parallel build settings
- a named volume for SQLite persistence
- explicit `DATABASE_URL` override inside the container

## Migration strategy

- migrations must remain additive
- snapshot the SQLite database before rollout
- if rollback is needed, restore the snapshot and redeploy the previous binary

## Operational checks after deployment

- create task from quick mode
- create task from guided mode
- assign task to a user who already started the bot
- assign task to a directory employee who has not started the bot
- verify comment / blocker / review / reassignment flows
- verify overdue and reminder jobs
- verify queue retry behavior

## Documentation map

- [README.md](./README.md)
- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [docs/memory.md](./docs/memory.md)
- [docs/quality-roadmap.md](./docs/quality-roadmap.md)
