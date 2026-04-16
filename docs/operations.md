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
