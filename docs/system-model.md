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
- `created -> cancelled`
- `sent -> in_progress`
- `sent -> blocked`
- `sent -> cancelled`
- `in_progress -> blocked`
- `in_progress -> in_review`
- `in_progress -> cancelled`
- `blocked -> in_progress`
- `blocked -> cancelled`
- `in_review -> in_progress`
- `in_review -> completed`
- `completed -> in_progress`

Disallowed examples:
- `cancelled -> cancelled`
- `completed -> completed`
- `cancelled -> in_progress`

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
- callback payload plus task version is the safety boundary for stale UI actions
- business duplicates are surfaced truthfully, not hidden behind success wording

## Assignee resolution rules

Resolution order:
1. exact Telegram username match
2. exact registered user match
3. employee directory match
4. recent or likely collaborators
5. ambiguity flow
6. assign later

If multiple candidates match:
- show a button-based choice flow
- offer `None of them`
- offer `Assign later`

If the assignee exists in directory but has never started the bot:
- create the task
- show delivery as waiting for registration
- do not pretend the message was delivered

## Public task codes

- user-facing references use codes like `T-0042`
- UUID stays internal
- `/status` and related flows must accept both public code and UUID for compatibility

## System invariants

- business state and delivery state are separate models
- task cards never lie about duplicate creation
- user-facing text belongs to presentation rendering, not domain
- a stale callback never performs a silent unsafe mutation
- dangerous actions always require confirmation
- list items always carry enough context to understand urgency
- human-friendly public task code is the default reference everywhere user-facing
- audit history should record meaningful changes only

## Priority roadmap

### Critical now

- truthful duplicate handling
- public task code everywhere in UX
- compact task card
- manager inbox
- focus screen
- action prioritization
- stale callback safety

### Second wave

- full button-based ambiguity resolution
- smart assignee suggestions
- richer blocker escalation
- digest tuning and quiet reminder policies
- deeper handler decomposition

