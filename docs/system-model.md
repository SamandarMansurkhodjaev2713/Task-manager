# System Model

This document captures the current target product and engineering model for the Telegram task bot.

## Primary user flows

### `/start`

The bot should guide the user to the next useful action within one screen:
- individual contributor:
  - `My focus`
  - `Create task`
  - `My tasks`
  - `Created by me`
  - `My stats`
  - `Profile`
- manager:
  - all individual actions
  - `Team tasks`
  - `Manager inbox`
  - `Team stats`

### Quick create

Use when the task is already formulated in one text or voice message.

Expected outcomes:
- `Created`
- `DuplicateFound`
- `ClarificationRequired`

### Voice create

Voice creation is a guarded flow:
1. receive voice
2. transcribe
3. show interpretation screen with description, assignee, deadline, and clarification risk
4. allow confirm, edit, or cancel
5. create only after explicit confirmation

### Registration and employee linking

Registration is separate from employee linking, but the bot tries to connect them safely:
- exact username match may auto-link
- exact unique full-name match may auto-link
- ambiguous candidates require explicit choice
- users may continue unlinked deliberately
- once linked, pending employee-assigned tasks are recovered automatically

### Guided create

Use when the author needs help with assignee, wording, or deadline.

Flow:
1. choose or type assignee
2. enter task summary
3. choose deadline or `No deadline`
4. review draft
5. confirm

### Task follow-up

The default task card is compact and action-oriented:
- public task code
- title
- status
- deadline
- assignee
- delivery
- delivery explanation
- blocker or review risk when relevant
- next best action

The expanded card is a details view:
- description
- expected result
- acceptance criteria
- latest comments
- latest history entries

## Task state machine

Business state is independent from delivery state.

Allowed task states:
- `created`
- `sent`
- `in_progress`
- `blocked`
- `in_review`
- `completed`
- `cancelled`

Allowed transitions:
- `created -> sent`
- `created -> in_progress`
- `created -> blocked`
- `created -> cancelled`
- `sent -> in_progress`
- `sent -> blocked`
- `sent -> in_review`
- `sent -> cancelled`
- `in_progress -> blocked`
- `in_progress -> in_review`
- `in_progress -> cancelled`
- `blocked -> in_progress`
- `blocked -> in_review`
- `blocked -> cancelled`
- `in_review -> in_progress`
- `in_review -> completed`
- `in_review -> cancelled`

Disallowed examples:
- `cancelled -> cancelled`
- `completed -> completed`
- `cancelled -> in_progress`
- `completed -> in_progress`

Repeated valid actions must be idempotent and should not create duplicate audit noise.

## Delivery state machine

Delivery states:
- `pending`
- `sent`
- `retry_pending`
- `failed`

User-facing meanings:
- `sent`: delivered
- `pending`: queued or waiting for registration
- `retry_pending`: retry scheduled
- `failed`: delivery failed after retries

Business state must never imply delivery success.

## Screen lifecycle

The Telegram UI uses a hybrid screen model:

- navigation screens are edited in place
- event notifications remain separate messages
- a safe fallback sends a new screen only when Telegram editing is unavailable

The bot tracks one active screen per chat. Stale callbacks from older messages are rejected for mutating actions and redirected safely for navigation actions.

## Authorization model

### Regular user

Can:
- create tasks
- view tasks assigned to them
- view tasks created by them
- comment where they participate
- move their own assigned task through allowed workflow states
- mark blockers on tasks they actively work on

Cannot:
- manage broad team views
- review unrelated tasks
- mutate tasks they do not participate in

### Manager

Can:
- access team views
- access manager inbox
- review tasks in their scope
- reassign tasks in their scope
- react to blockers in their scope

### Admin

Can:
- perform operational admin actions
- manage employee sync
- keep all manager permissions

Row-level authorization is enforced in application policies, not only at Telegram entry points.

## Idempotency rules

### Creation

- quick create dedupes by source message key
- guided create uses a synthetic stable source key for the confirmed draft
- duplicate detection returns `DuplicateFound`, never fake success

### Status changes

- repeated callback with the same source action must be safe
- stale callback must resolve to a fresh card or a clear explanation
- if the state already matches the requested terminal state, no duplicate audit record should be written

### Dangerous actions

- cancel requires explicit confirmation
- repeated confirmation must be safe

## Dedupe rules

- source message key is the primary dedupe mechanism for intake
- active-screen message identity is the current stale-UI safety boundary for callback handling
- business duplicates are surfaced truthfully, not hidden behind success wording

## Assignee resolution rules

Resolution order:
1. exact Telegram username match
2. exact employee directory match
3. exact first name only when unique
4. explicit ambiguity flow for all non-safe cases

If multiple candidates match:
- return clarification
- show likely candidates as explicit buttons

If a full name is misspelled or fuzzy:
- do not auto-assign
- require clarification or a deliberate unassigned decision

If the assignee exists in directory but has never started the bot:
- create the task
- show delivery as waiting for registration
- show a dedicated help path for forwarding a simple `/start` instruction
- do not pretend the message was delivered

## Public task codes

- user-facing references use codes like `T-0042`
- UUID stays internal
- `/status` and related flows accept both public code and UUID for compatibility

## System invariants

- business state and delivery state are separate models
- task cards never lie about duplicate creation
- user-facing text belongs to presentation rendering, not domain
- a stale callback never performs a silent unsafe mutation
- dangerous actions always require confirmation
- list items always carry enough context to understand urgency
- human-friendly public task code is the default reference everywhere user-facing
- audit history records meaningful changes only
- navigation flows should not spam the chat
- voice intake never creates a task before explicit confirmation
- registration never auto-links on low-confidence identity matching
- unresolved employee ambiguity never silently creates a wrong assignment

## Priority roadmap

### Critical now

- truthful duplicate handling
- public task code everywhere in UX
- compact task card
- manager inbox
- focus screen
- action prioritization
- stale callback safety
- edit-in-place navigation

### Second wave

- full button-based ambiguity resolution
- smart assignee suggestions
- richer blocker escalation
- digest tuning and quiet reminder policies
- deeper handler decomposition
- version-aware stale callback protection
