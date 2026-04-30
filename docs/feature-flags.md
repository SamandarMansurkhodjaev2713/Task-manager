# Feature Flag Registry

**Purpose:** Single source of truth for all feature flags. Every flag must have an entry
here documenting its status, default, owner intent, and what code reads it.

**Rule:** A flag that is declared but never read outside tests for more than one release
cycle must either be implemented or deleted. Stranded declarations are a maintenance liability.

---

## Flag status definitions

| Status | Meaning |
|--------|---------|
| **LIVE** | Flag is read in production code; behavior is gated. |
| **DEFAULT ON** | Enabled by default in `FeatureFlag::default_enabled()`. |
| **DEFAULT OFF** | Not in default_enabled; must be toggled via ENV or admin panel. |
| **SKELETON** | Flag is declared and parsed, but no production code reads it. Behavior exists partially or not at all. |

---

## All flags

### `onboarding_v2`
- **Status:** LIVE · DEFAULT ON
- **Read in:** `dispatcher_handlers::run_onboarding_gate` — gates all traffic for users with
  incomplete onboarding through the v2 FSM (FirstName → LastName → EmployeeLink → Complete).
- **Intent:** Ships enabled. Disabling reverts to legacy direct-registration flow.
- **Owner:** Core UX.

### `admin_panel`
- **Status:** LIVE · DEFAULT ON
- **Read in:** Every admin dispatcher function (show_admin_menu, show_admin_users,
  admin user detail/role/deactivate flows, audit log, feature toggles).
- **Intent:** Ships enabled. Can be disabled to hide admin surfaces during maintenance.
- **Owner:** Admin / Ops.

### `team_analytics`
- **Status:** LIVE · DEFAULT ON
- **Read in:** `dispatcher_handlers` — gates `BotCommand::TeamStats` and
  `TelegramCallback::MenuTeamStats`. Managers and Admins see team stats when enabled.
- **Intent:** Ships enabled. Managers need team stats for their daily work (role matrix
  grants access unconditionally; this flag exists only for emergency disable).
- **Owner:** Manager UX.

### `sla_escalations`
- **Status:** LIVE · DEFAULT OFF
- **Read in:** `UpdateSlaStatesUseCase::execute` — background job runs every 5 minutes;
  updates `sla_state` on tasks and enqueues `SlaEscalation` notifications.
- **Intent:** Disabled until SLA noise calibration and pilot validation complete.
  Enable once pilot team accepts SLA alert volume and content.
- **Owner:** Manager UX.

### `recurrence_rules`
- **Status:** LIVE · DEFAULT OFF
- **Read in:** `ProcessRecurrenceRulesUseCase::execute` — background job runs every 60s;
  fires due recurrence rules; creates tasks from templates.
- **Intent:** Scheduler backend is complete. Disabled until UI for creating/managing
  recurrence rules ships (Phase 5 / Etap F).
- **Owner:** Task creation.

### `voice_v2`
- **Status:** SKELETON · DEFAULT OFF
- **Read in:** Not read in production code. Voice callbacks (VoiceCreateConfirm/Edit/Back/Cancel)
  run unconditionally — the flag previously gated them but was removed in Phase 1 to unblock voice.
- **Intent:** Reserved for Phase 3 voice enhancements (single-confirmation screen,
  smarter transcript editing). Wire to those changes when implemented, or delete.
- **Action required:** Wire the flag to guard Phase 3 voice changes when implemented,
  or delete if the voice flow will be changed unconditionally.

### `task_templates`
- **Status:** SKELETON · DEFAULT OFF
- **Read in:** Not found in production code. Schema, domain types, and recurrence
  scheduler reference templates, but no UI handler reads this flag.
- **Intent:** Gate templates UI when implemented (Phase 5 / Etap F).
- **Action required:** Wire flag check to template list/use UI when those handlers ship.

---

## Removed flags (Phase 1 cleanup)

The following flags were removed from the enum in Phase 1. Existing `feature_flag_overrides`
rows with these keys are silently ignored (the parser emits a warning and continues).

| Key | Reason removed |
|-----|----------------|
| `inline_assignee_search` | DEAD — never read in production. Remove and re-add when Phase 2 inline search ships. |
| `notification_digest` | DEAD — never read in production. Remove and re-add when Phase 4 batching ships. |
| `csv_export` | DEAD — never read in production. Remove and re-add when Phase E ops hardening ships. |

---

## Operational notes

- Feature flags are toggled at runtime via the admin panel (`/admin` → Флаги функций).
- Overrides persist to `feature_flag_overrides` table (migration 008).
- The ENV variable `FEATURE_FLAGS` sets the baseline (comma-separated keys).
- Unknown flag names in ENV or DB emit a warning log and are ignored — they never panic.
- The `SharedFeatureFlagRegistry` is an `Arc<RwLock<FeatureFlagRegistry>>` shared across
  all handlers; writes happen only on admin toggles, reads are nearly always contention-free.
