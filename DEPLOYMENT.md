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
3. Populate `TELEGRAM_ADMIN_IDS` with the Telegram IDs of people who
   must be admins from day one.  The list is additive and
   promotion-only; the last-admin invariant is enforced by the
   repository, so deletion from `.env` never demotes anyone.
4. Verify `FEATURE_FLAGS` only lists known flags (the process refuses
   to boot on unknown names to surface typos early).
5. Mount persistent storage for SQLite
6. Run migrations on startup
7. Ask employees to send `/start` to the bot at least once
8. Verify observability endpoints:
   - `GET /healthz` — must return `200` with `{"status":"ok"}`
   - `GET /healthz/deep` — must return `200` with `{"checks":{"database":{"status":"ok",…}}}`
   - `GET /metrics` — must serve Prometheus text with content-type
     `text/plain; version=0.0.4`
   - `GET /version` — cross-check `version` and `git_sha` against the
     image tag being rolled out
9. Check that the notification queue drains successfully

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

Migrations must remain additive (no destructive schema changes except via explicit
table-recreation migrations like 005).

### Rollback procedure

SQLite does not support `DOWN` migrations.  The rollback procedure is snapshot-based:

**Before every release:**

```bash
# 1. Stop the bot (ensures no writes during copy)
docker compose stop telegram-task-bot

# 2. Snapshot the database
SNAPSHOT_NAME="app_$(date +%Y%m%dT%H%M%S).db.bak"
cp /path/to/data/app.db "/path/to/backups/${SNAPSHOT_NAME}"
echo "Snapshot saved: ${SNAPSHOT_NAME}"

# 3. Deploy the new version
docker compose pull telegram-task-bot
docker compose up -d telegram-task-bot
```

**If rollback is needed:**

```bash
# 1. Stop the new version
docker compose stop telegram-task-bot

# 2. Restore the snapshot (overwrites the migrated database)
cp "/path/to/backups/${SNAPSHOT_NAME}" /path/to/data/app.db

# 3. Redeploy the previous image tag
docker compose up -d telegram-task-bot  # ensure COMPOSE_IMAGE_TAG points to previous tag
```

**Constraints:**
- Only safe if the new schema migration is *additive*.  Recreating-table migrations
  (e.g., 005) that preserve all row data are also safe to roll back via snapshot.
- Any data written *after* the snapshot (new tasks, comments, etc.) is lost.
  For a small team this window is typically < 5 minutes; communicate before rollout.
- If you cannot afford data loss, export affected rows before stopping and re-import
  after restoring the snapshot.

## Operational checks after deployment

- send `/start` from a brand-new Telegram account and complete the
  onboarding FSM (first name → last name → optional employee link)
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
- verify `/admin` panel with a bootstrap admin:
  - list users and open a user card
  - change role (confirm via nonce button) and check `admin_audit_log`
  - attempt to deactivate the last admin — must be refused
  - attempt to change own role — must be refused
  - confirm expired nonce message when waiting > TTL
- verify `/find <запрос>` returns a bounded list of matching tasks

## Admin lockout recovery

The system is designed to make full lockout impossible via UI flows
(last-admin invariant + self-target guard).  If lockout still happens
(e.g. the `.env` list was emptied and the only admin's account got
deleted externally), recover as follows:

1. Stop the bot container.
2. Add the rescue Telegram ID to `TELEGRAM_ADMIN_IDS` in `.env`.
3. Start the bot.  `BootstrapAdminsUseCase` will promote the rescue
   account on the next process start; if the user has not sent `/start`
   yet, their elevation is recorded as pending and applied on first
   interaction.
4. Verify the elevation through `/admin → журнал аудита` and rotate
   the rescue ID back out of `.env` afterwards (demotion must be done
   from inside the admin panel).

## Documentation map

- [README.md](./README.md)
- [ARCHITECTURE.md](./ARCHITECTURE.md)
- [docs/memory.md](./docs/memory.md)
- [docs/operations.md](./docs/operations.md)
- [docs/quality-roadmap.md](./docs/quality-roadmap.md)
