# Architecture

## Layer model

- `presentation`
  - Telegram dispatcher, callback routing, UI text and keyboards
  - HTTP health and metrics endpoints
- `application`
  - use cases
  - repository and service contracts
  - orchestration logic
- `domain`
  - task lifecycle
  - notification delivery lifecycle
  - parsing rules
  - business invariants
  - value objects and entities
- `infrastructure`
  - SQLite repositories
  - Telegram notifier
  - AI providers
  - employee directory integration
  - scheduler
- `shared`
  - constants and neutral helpers

## Core architectural rules

- Domain is pure:
  - no Teloxide
  - no SQLx
  - no reqwest
  - no direct IO
- Application orchestrates and enforces use-case rules
- Infrastructure owns all side effects
- Presentation is thin and should not contain business rules

## Core business concepts

### Task business lifecycle

- `created`
- `sent`
- `in_progress`
- `blocked`
- `in_review`
- `completed`
- `cancelled`

### Notification delivery lifecycle

- `pending`
- `sent`
- `retry_pending`
- `failed`

Important: these are separate models on purpose.

## Design decisions

### Idempotent task creation

- tasks are deduplicated by `source_message_key`
- guided flow uses a stable synthetic source key
- duplicate submits should open the existing task instead of creating a second copy

### Centralized assignee resolution

- all assignment logic should go through `AssigneeResolver`
- ambiguity should trigger clarification, not guesswork
- "assignee exists but has never started the bot" is handled as a valid state

### Centralized task actions

- valid transitions come from the domain state machine
- available actions come from application policy, not ad hoc UI checks
- Telegram buttons should reflect only currently valid actions

### Optimistic locking

- `tasks.version` protects concurrent updates
- callback races and multi-actor updates should fail safely instead of silently overwriting state

### Notification queue

- notification sending is asynchronous
- enqueue is not treated as delivery success
- failed sends are visible through delivery state

## Current known architectural debt

- several files remain too large and should be split further
- repository implementation is still too concentrated in one module
- Telegram presentation is cleaner than before, but still larger than ideal in some areas

## Documentation map

- [README.md](./README.md)
- [DEPLOYMENT.md](./DEPLOYMENT.md)
- [docs/memory.md](./docs/memory.md)
- [docs/quality-roadmap.md](./docs/quality-roadmap.md)
