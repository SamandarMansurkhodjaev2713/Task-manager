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

UUID remains internal and still works for backward compatibility, but the default UX should use the public code everywhere.

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
- next best action
- short task preview

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

### Guided create

Use when:
- the assignee is unclear
- you want to avoid missing deadline/details
- you want safer structured input

## Duplicate behavior

Duplicate detection must be truthful.

If the same source message was already processed:
- do not say “task created”
- explain that the existing task was found
- open the existing card

## Delivery visibility

Delivery state must be visible separately from business state.

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
- the interface should help the user decide what to do next in seconds
