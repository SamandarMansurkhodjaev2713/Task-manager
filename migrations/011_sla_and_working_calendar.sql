-- 011_sla_and_working_calendar.sql
--
-- Phase 1 — SLA / escalation machinery + per-organisation working hours.
--
-- Rationale:
-- * `sla_state` on tasks is a denormalised field driven by a background
--   job so that the admin panel and notifications can filter by status
--   without recomputing from deadlines on every request.  Default `null`
--   means "no SLA tracking" for legacy rows.
-- * `sla_escalations` records each escalation the task went through so
--   we can:
--      - stop re-escalating the same level (idempotency, CAS on
--        `last_level`);
--      - render the escalation timeline in the task card.
-- * `working_calendars` models a per-tenant calendar.  For v3 we are a
--   single-tenant bot but we still normalise the schema so future
--   multi-tenant evolution is a matter of adding a `tenant_id` FK, not a
--   data migration.
-- * `working_calendar_holidays` holds one-off overrides (holidays,
--   work-on-weekend days).
--
-- SAFETY: additive.  The single ALTER on `tasks` adds a nullable column
-- and keeps the existing row layout compatible with older binaries.

PRAGMA foreign_keys = ON;

ALTER TABLE tasks ADD COLUMN sla_state TEXT;
ALTER TABLE tasks ADD COLUMN sla_last_level INTEGER NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_tasks_sla_state ON tasks (sla_state);

CREATE TABLE IF NOT EXISTS sla_escalations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    level INTEGER NOT NULL,
    triggered_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    actor TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '{}',
    UNIQUE (task_id, level),
    FOREIGN KEY (task_id) REFERENCES tasks (id)
);

CREATE INDEX IF NOT EXISTS idx_sla_escalations_task_id ON sla_escalations (task_id);

CREATE TABLE IF NOT EXISTS working_calendars (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    timezone TEXT NOT NULL,
    workday_mask INTEGER NOT NULL DEFAULT 31,
    workday_start_min INTEGER NOT NULL DEFAULT 540,
    workday_end_min INTEGER NOT NULL DEFAULT 1080,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS working_calendar_holidays (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    calendar_id INTEGER NOT NULL,
    day TEXT NOT NULL,
    is_working INTEGER NOT NULL DEFAULT 0,
    note TEXT,
    FOREIGN KEY (calendar_id) REFERENCES working_calendars (id),
    UNIQUE (calendar_id, day)
);
