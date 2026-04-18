-- Migration 005: Remove UNIQUE constraint on employees.full_name.
--
-- The original UNIQUE(full_name) breaks employee sync when two people share the
-- same name (common in practice).  SQLite does not support ALTER TABLE … DROP
-- CONSTRAINT, so we recreate the table.
--
-- Replacement strategy:
--   • full_name is NOT UNIQUE — same-name employees can coexist.
--   • telegram_username keeps a PARTIAL UNIQUE index (WHERE telegram_username IS NOT NULL)
--     so one Telegram account cannot be linked to two employee records.
--   • Tasks that reference employees.id are unaffected — we preserve all IDs.

PRAGMA foreign_keys = OFF;

CREATE TABLE employees_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name TEXT NOT NULL,
    telegram_username TEXT,
    email TEXT,
    phone TEXT,
    department TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    synced_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO employees_new
    SELECT id, full_name, telegram_username, email, phone, department,
           is_active, synced_at, created_at, updated_at
    FROM employees;

DROP TABLE employees;

ALTER TABLE employees_new RENAME TO employees;

-- Plain index for name-based lookups (no uniqueness).
CREATE INDEX IF NOT EXISTS idx_employees_name ON employees (full_name);

-- Partial unique index: prevents the same Telegram account from mapping to two
-- different employee records, while still allowing rows with NULL telegram_username.
CREATE UNIQUE INDEX IF NOT EXISTS idx_employees_telegram_username_unique
    ON employees (telegram_username)
    WHERE telegram_username IS NOT NULL;

PRAGMA foreign_keys = ON;
