# Current Functionality Contract

**Purpose:** Ground truth of what the bot actually does today. This file must not drift
from the codebase. When a feature ships or is removed, update this file in the same PR.

**Last verified:** 2026-05-06 against local Docker/Linux test gate.

---

## Commands

| Command | Payload | Role | What it does |
|---------|---------|------|-------------|
| `/start` | — | None | Onboarding gate; links user to employee or registers new |
| `/menu` | — | User | Opens main menu |
| `/help` | — | User | Opens help screen |
| `/new_task` | optional text | User | Quick-capture; pre-fills from payload if present |
| `/my_tasks` | optional cursor | User | Assigned tasks (paginated) |
| `/created_tasks` | optional cursor | User | Created-by-me tasks (paginated) |
| `/team_tasks` | optional cursor | Manager/Admin | All team tasks |
| `/status` | task_uid (required) | Creator/Assignee/Manager/Admin | Open task card |
| `/cancel_task` | task_uid (required) | Creator/Assignee/Admin | Cancel confirmation |
| `/stats` | — | User | Personal task statistics |
| `/team_stats` | — | Manager/Admin | Team statistics |
| `/settings` | — | User | Profile and notification settings |
| `/admin_sync_employees` | — | Admin | Trigger Google Sheets employee sync |
| `/admin` | — | Admin | Admin panel |
| `/find` | optional query | User | **Skeleton only — returns placeholder response. No real search.** |

---

2026-05-06 correction: `/find` now searches the caller's visible tasks and returns a bounded matching list; it is no longer a placeholder-only command.

## Task statuses and transitions

```
Created → {Sent, InProgress, Blocked, Cancelled}
Sent    → {InProgress, Blocked, InReview, Cancelled}
InProgress → {Blocked, InReview, Cancelled}
Blocked    → {InProgress, InReview, Cancelled}
InReview   → {InProgress, Completed, Cancelled}
Completed  → (terminal)
Cancelled  → (terminal)
```

**Review gate:** if the task has an `expected_result`, an assignee requesting
`Completed` is silently redirected to `InReview`. Only creator or admin can
approve `InReview → Completed`.

---

## Active-screen model

One `ActiveScreenState { message_id, descriptor }` per `chat_id`, stored in memory.

A callback is **stale** when its `message_id` does not match the stored one.
Stale navigational callbacks show "Этот экран уже устарел" and open a fresh screen.
Stale mutating callbacks show a safe error message and make no state change.

---

## Create flows

### Quick create
1. User sends text (or `/new_task <text>`) or voice message.
2. Bot runs AI parsing → extracts title, assignee, deadline.
3. If assignee match is `Unique` (confidence ≥ 95) → auto-resolve.
4. If match is `Suggested` or `Ambiguous` → clarification keyboard.
5. If `NotFound` → unassigned (allowed for quick create).
6. Duplicate check via `source_message_key`; honest duplicate card if found.
7. Task created → TaskCreationResult screen.

### Guided create (4 steps)
1. Assignee step — text input with AI resolution + candidate keyboard.
2. Title/description step — free text.
3. Deadline step — free text or skip.
4. Confirmation step — summary + submit/edit/cancel.

### Voice create
1. **Processing** screen shown immediately.
2. Audio transcribed via AI service (async).
3. On success → **Confirmation** screen: raw transcript + AI interpretation (assignee, deadline, description) + buttons [Создать / Исправить / Назад].
4. If assignee is ambiguous → **GuidedAssigneeOptions** screen inserted before final confirmation.
5. If user taps "Исправить" → **Edit** screen: bot awaits new text.
6. After edit → back to Confirmation.
7. Confirm → **TaskCreationResult** screen.
8. On transcription failure → error message + return to creation menu.

> **Doc vs. runtime gap:** `docs/telegram-ux.md` describes "one confirmation screen."
> The runtime has Processing + Confirmation as two mandatory screens, plus two optional
> ones (GuidedAssigneeOptions, Edit). The single-screen target is a Phase 3 goal, not
> current state.

---

## Assignee resolution chain

Executed in order; first conclusive match wins.

| Priority | Strategy | Confidence | Outcome |
|----------|----------|-----------|---------|
| 1 | Exact `@username` | 100 | Unique |
| 2 | Exact full name (normalized) | 100 | Unique or Ambiguous |
| 3 | Exact first name (single-word query) | 100 | Unique or Ambiguous |
| 4 | Prefix on first name (≥ 2 chars) | 78 | Unique or Ambiguous |
| 5 | Levenshtein fuzzy on first name | 60–94 | Suggested or Ambiguous |
| 6 | Levenshtein fuzzy on full name | 60–94 | Suggested or Ambiguous |

**Outcome → UX:**
- `Unique` (≥ 95) → auto-assign, no confirmation prompt.
- `Suggested` (75–94) → "Вы имели в виду — X?" with one confirm button + alternatives.
- `Ambiguous` (multiple ≥ 75) → candidate keyboard, user picks one.
- `NotFound` (all < 75) → unassigned fallback or clarification.

**Person trigrams:** schema and index exist (`person_trigrams` table, migration 009).
Not used in production matching today. Reserved for future `/find` v2 or fuzzy backend.

---

## Notifications

| Type | Trigger | Sent to |
|------|---------|---------|
| `TaskAssigned` | Assignee set on task | Assignee |
| `TaskUpdated` | Task fields change | **Not wired — no notification sent today** |
| `DeadlineReminder` | Scheduled daily job (upcoming + overdue) | Assignee |
| `TaskCompleted` | Status → Completed | Creator |
| `TaskCancelled` | Status → Cancelled | Assignee and creator |
| `TaskReviewRequested` | Status → InReview | Creator |
| `TaskBlocked` | Status → Blocked | Creator |
| `DailySummary` | Scheduled daily job | All active users with tasks |
| `SlaEscalation` | Task enters at_risk or breached | Creator and assignee |

Delivery: exponential backoff (60s → 120s → 240s), dedup keys, permanent-fail codes.
Concurrent processing: 10 in-flight.

---

## Background jobs

| Job | Interval | What it does |
|-----|---------|-------------|
| Employee sync | Configurable (env) | Pulls Google Sheets directory; upserts employees |
| Notification processor | Configurable (env) | Sends pending notifications via Telegram |
| Deadline reminders | Daily at configured UTC hour | Enqueues reminders for tasks due tomorrow |
| Overdue alerts | Daily at configured UTC hour | Enqueues alerts for overdue tasks |
| Daily summary | Daily at configured UTC hour | Enqueues daily summary for active users |
| SLA scan | Every 5 minutes | Updates SLA states; enqueues escalation notifications (gated by `SlaEscalations` flag) |
| Recurrence processor | Every 60 seconds | Fires due recurrence rules; creates tasks from templates (gated by `RecurrenceRules` flag) |

---

## Features that exist only as backend skeleton (no UI)

2026-05-06 correction: `/find` should no longer be treated as a backend-only skeleton; it has live search behavior.

| Feature | What exists | What is missing |
|---------|------------|-----------------|
| Task templates | Schema, domain types, repository | Any UI for create/list/use |
| Recurrence rules | Schema, domain types, scheduler | Any UI for create/pause/resume; tasks created by recurrence have no assignee |
| `/find` | Command parsing | Real search implementation |
| Inline assignee search | Feature flag declared | Implementation (flag never read) |
| Team analytics | Feature flag declared | Implementation (flag never read) |
| CSV export | Feature flag declared | Implementation (flag never read) |
| Notification digest/batching | Feature flag declared | Implementation (flag never read) |
| Task audit log | — | Schema, domain types, repository, UI |
| Assignee history / personal boost | — | Schema, domain types, repository, use case |
| Employee aliases / abbreviations | — | Everything |

---

## Employee source model

Two sources: `google_sheets` and `bot_registered`.

- `upsert_many` (Sheets sync): always writes `source = 'google_sheets'`; upgrades `bot_registered` row for the same username.
- `upsert_bot_registered` (onboarding): inserts `source = 'bot_registered'`; deduplicates by `telegram_username`; returns existing row if username already exists.
- Both sources are visible to the assignee resolver and appear in candidate suggestions.
