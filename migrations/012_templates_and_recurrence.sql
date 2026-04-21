-- 012_templates_and_recurrence.sql
--
-- Phase 1 — templates + recurring tasks scaffolding.
--
-- Rationale:
-- * Templates are a first-class feature in the v3 plan — admins create
--   re-usable task shells, users spawn tasks from them.  We store the
--   entire template as canonical JSON in `body` so the same template can
--   evolve with new fields without schema churn.
-- * `recurrence_rules` models a CRON-like recurrence against a template
--   or a raw task body.  `next_run_at` is denormalised from the rule so
--   the scheduler can pick due rows with a fast index scan.  The rule
--   itself is a plain CRON string (validated at write time by the
--   `cron` crate).
--
-- SAFETY: new tables only.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS task_templates (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    code TEXT NOT NULL UNIQUE,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_by_user_id INTEGER,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (created_by_user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_task_templates_is_active ON task_templates (is_active);

CREATE TABLE IF NOT EXISTS recurrence_rules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    template_id INTEGER,
    owner_user_id INTEGER NOT NULL,
    cron_expression TEXT NOT NULL,
    timezone TEXT NOT NULL DEFAULT 'Europe/Moscow',
    next_run_at TEXT,
    last_run_at TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (template_id) REFERENCES task_templates (id),
    FOREIGN KEY (owner_user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_recurrence_rules_next_run_at
    ON recurrence_rules (is_active, next_run_at);
CREATE INDEX IF NOT EXISTS idx_recurrence_rules_owner
    ON recurrence_rules (owner_user_id);
