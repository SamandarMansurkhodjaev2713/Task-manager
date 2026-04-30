# Screen Lifecycle Contract

**Purpose:** Formal rules for when the bot edits an existing message vs. sends a new one.
Every dispatcher handler and keyboard action must follow these rules without exception.

---

## Screen categories

### RootScreen
The "home" of a navigation context. Each user has exactly one active root screen at a time.
Navigating to a new root screen sends a new message and records its `message_id` as active.

| Root Screens |
|-------------|
| MainMenu |
| CreateMenu |
| TaskList (any origin) |
| TaskDetail |
| AdminMenu |
| OnboardingFirstName |
| OnboardingLastName |
| RegistrationLinking |
| Settings |
| Stats |

### FlowScreen
A step within a multi-step flow anchored to the current root. Edited in place
on the same message as the root until the flow completes.

| Flow Screens |
|-------------|
| GuidedStep(Assignee / Description / Deadline) |
| GuidedAssigneeOptions |
| GuidedConfirmation |
| VoiceCreate(Confirmation) |
| VoiceCreate(Edit) |
| TaskInteractionPrompt (comment / blocker / reassign) |
| CancelConfirmation |
| DeliveryHelp |
| AdminUserDetails |
| AdminConfirm (nonce) |
| AdminFeatures |
| AdminUsers |
| AdminAudit |
| AdminSecurityAudit |

### TerminalScreen
Displayed after a flow produces a result. Offers CTAs back to a root context.
**Must never be reused as a container for a new flow.** Any action on a
TerminalScreen that starts a new root flow sends a fresh new message.

| Terminal Screens |
|-----------------|
| TaskCreationResult |
| SyncEmployeesResult |

### EventMessage
Async notifications that are always separate messages, never edits.
These never overwrite the active screen.

| Event Messages |
|---------------|
| TaskAssigned notification |
| DeadlineReminder notification |
| TaskCompleted notification |
| TaskCancelled notification |
| TaskReviewRequested notification |
| TaskBlocked notification |
| DailySummary notification |
| SlaEscalation notification |

---

## Edit vs. fresh-send policy

### Edit in place (same `message_id`)
Use `bot.edit_message_text()` when transitioning between:
- Any two screens in the same root context where the root message is still active.
- Any FlowScreen within a flow.
- Root → FlowScreen (e.g., MainMenu → CreateMenu, CreateMenu → GuidedStep).
- FlowScreen → FlowScreen within the same flow.
- Pagination within a TaskList.
- Compact ↔ Expanded TaskDetail.

### Fresh send (new message)
Use `bot.send_message()` when:
- Starting a completely new root context after a TerminalScreen.
- A stale callback is detected → close with error + open fresh current root.
- Fallback when Telegram rejects the edit (message too old, media type change).
- Opening the first screen after `/start` or `/menu`.
- An EventMessage (never edits — always a separate message).

### "New flow from TerminalScreen" rule
```
User taps "Ещё задача" on TaskCreationResult
  → send_message() with CreateMenu   [new message, new active screen]
  → record new message_id as active

User taps "В меню" on TaskCreationResult
  → send_message() with MainMenu     [new message, new active screen]
```
The TerminalScreen message is left as-is in chat history. It is NOT edited.

---

## Stale callback policy

A callback is stale when its `message_id` ≠ the stored active screen `message_id`.

| Callback type | Stale action |
|--------------|-------------|
| Navigational (no state change) | Show "Этот экран уже устарел" toast via `answer_callback_query`; open fresh current root screen. |
| Mutating (state change) | Show error toast; make **no** state change; do **not** open a new screen. |

**Key invariant:** A stale mutating callback never mutates state. Period.
This prevents double-tap on "Создать задачу" creating two tasks.

---

## Active screen invariants

1. **One active root screen per chat at all times.** The `ActiveScreenStore` maps
   `chat_id → ActiveScreenState { message_id, descriptor }`.
2. **EventMessages do not update `ActiveScreenStore`.** They are sent with
   `send_message()` but the stored active screen is not changed.
3. **Terminal screens are recorded as active** so their buttons are stale-safe.
   Their message_id is stored; any callback on them is processed normally.
4. **Stage → allowed callbacks.** Each `Stage` has an explicit allowlist of
   `TelegramCallback` variants. A callback not in the allowlist for the current
   stage is silently rejected with a toast.

---

## Progress indication (Phase 1 target)

Every multi-step FlowScreen must show a header indicating position:

```
Шаг 2/4 · Описание задачи
```

This applies to: GuidedStep (all 4 steps), VoiceCreate (Confirmation / Edit are steps 2/3).

---

## Terminal screen CTAs (required)

Every TerminalScreen must have at minimum:
- "Открыть задачу" (if a task was created/modified)
- "Создать ещё" (for creation terminals)
- "В меню" (always)

Tapping "В меню" or "Создать ещё" sends a fresh new root message. The terminal
message becomes historical chat context.
