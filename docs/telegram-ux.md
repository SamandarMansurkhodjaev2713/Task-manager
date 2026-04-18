# Telegram UX

This document describes the current target UX model for the bot.

## Main menu

Personal navigation:
- `Мой фокус`
- `Создать задачу`
- `Мои задачи`
- `Созданные мной`
- `Моя статистика`
- `Профиль`

Manager-only navigation:
- `Командные задачи`
- `Inbox менеджера`
- `Командная статистика`

## Public task reference

User-facing task identifiers are short codes like:
- `T-0001`
- `T-0042`
- `T-1543`

UUID remains internal and still works for backward compatibility, but the default UX uses the public code everywhere.

## Screen lifecycle

Navigation inside the bot follows a hybrid model:

- screen navigation is edited in place
- event notifications remain separate messages
- fallback to a new screen message happens only when Telegram editing is unavailable

### Edited in place

- main menu
- help
- profile/settings
- `Мой фокус`
- `Мои задачи`
- `Созданные мной`
- `Командные задачи`
- `Inbox менеджера`
- task list pagination
- compact and expanded task card
- cancel confirmation
- guided create steps
- quick create prompt
- comment, blocker, and reassign prompts

### Sent as separate messages

- task assignment notifications
- reminders
- daily digests
- overdue alerts
- review requested notifications
- blocker escalation notifications
- reassignment notifications

## Task card

Default mode:
- compact
- shows only the information needed to act

Compact card contains:
- title
- public task code
- status
- deadline
- assignee
- delivery status
- short delivery explanation
- current blocker or risk note when it matters
- next best action
- short task preview
- optional success/error notice banner at the top

If delivery is waiting for assignee registration:
- the compact card explains that the task is already saved
- the card offers a dedicated help screen with a ready-to-forward instruction for the employee

Expanded card contains:
- full task description
- expected result
- acceptance criteria
- latest comments
- latest history entries

## List screens

### My focus

This is the primary daily-use screen for an individual contributor.

Sections:
- urgent now
- waiting for my action
- blocked
- in review
- the rest in progress

### My tasks

This is the full assigned backlog with more complete grouping.

### Created by me

This is the author view for tracking delegated work.

### Team tasks

This is the broad manager overview.

### Manager inbox

This is the narrow decision-oriented screen for managers.

Sections:
- waiting for manager decision
- blockers
- assignee has not started bot yet
- deadline risk
- stale tasks without movement

## Creation flow

### Quick create

Use when:
- the task is already formulated
- speed is more important than control
- voice or one-shot text is convenient

### Voice create

Voice is a first-class creation flow:

1. user sends a voice message
2. the bot transcribes it
3. the bot shows one confirmation screen with the interpreted description, assignee, deadline, and any ambiguity warning
4. the user can confirm, edit, or cancel
5. only then is the task created

If the interpreted assignee is unsafe:
- the bot does not create the task yet
- it asks the user to choose the exact person explicitly
- the user can still create deliberately without assignee when that path is allowed

## Registration linking

If `/start` cannot safely link the Telegram account to exactly one employee:
- the bot opens a dedicated linking screen
- it shows explicit candidate buttons
- it allows continuing without linking only as a deliberate choice
- it never auto-links “close enough” employee identities

### Guided create

Use when:
- the assignee is unclear
- you want to avoid missing deadline/details
- you want safer structured input

## Duplicate behavior

Duplicate detection is truthful.

If the same source message was already processed:
- the bot does not say “task created”
- it explains that the existing task was found
- it offers the existing card

## Delivery visibility

Delivery state is shown separately from business state.

Possible user-facing meanings:
- delivered
- queued
- waiting for `/start`
- retrying
- failed
- author-only

## UX principles

- one screen = one main job
- short, live, direct text
- no machine labels in user-facing copy
- dangerous actions require confirmation
- stale buttons must fail safely
- the interface should help the user decide what to do next in seconds
- the chat should not be polluted by repeated menu and card messages
