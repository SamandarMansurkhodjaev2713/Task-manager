-- 015_employee_source.sql
--
-- Adds a `source` discriminator column to the `employees` table so the system
-- can distinguish Google-Sheets-synced entries from employees who registered
-- directly via the bot's /start onboarding flow.
--
-- Design:
--   • All existing rows receive `source = 'google_sheets'` (safe default — they
--     were all inserted by the Sheets sync).
--   • The Google Sheets sync (`SyncEmployeesUseCase`) always writes
--     `source = 'google_sheets'` in every INSERT and UPDATE, which upgrades
--     any `bot_registered` row whose owner later appears in Sheets.
--   • The onboarding flow creates `bot_registered` rows only when a user
--     completes /start without linking to any existing Sheets entry, ensuring
--     they are still addressable for task assignment.
--
-- SAFETY: ADD COLUMN with a NOT NULL DEFAULT is safe in SQLite ≥ 3.37 because
-- the engine fills the column lazily without rewriting existing rows.
-- No existing data is lost; CHECK guard prevents invalid values at insertion time.

PRAGMA foreign_keys = ON;

ALTER TABLE employees
    ADD COLUMN source TEXT NOT NULL DEFAULT 'google_sheets'
        CHECK (source IN ('google_sheets', 'bot_registered'));

CREATE INDEX IF NOT EXISTS idx_employees_source ON employees (source);
