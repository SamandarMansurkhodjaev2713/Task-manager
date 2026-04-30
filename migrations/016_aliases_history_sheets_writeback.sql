-- Migration 016: Employee aliases, assignee history, and sheets write-back queue.
--
-- All changes are ADDITIVE (no existing tables or columns modified).
-- Rollback: DROP TABLE IF EXISTS pending_sheet_writes, assignee_history, employee_aliases.

PRAGMA foreign_keys = ON;

-- ─── employee_aliases ────────────────────────────────────────────────────────
-- Maps short forms / diminutives / abbreviations to employees.
-- E.g. alias "Ваня" → employee_id for "Иван Иванов".
-- The unique index on lower(alias) prevents two aliases that are the same when
-- case-folded.  Two employees sharing the same alias produces an Ambiguous
-- outcome in the resolver — never a silent wrong assignment.
CREATE TABLE IF NOT EXISTS employee_aliases (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    employee_id        INTEGER NOT NULL,
    alias              TEXT    NOT NULL,
    created_by_user_id INTEGER,
    created_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    FOREIGN KEY (employee_id)        REFERENCES employees (id) ON DELETE CASCADE,
    FOREIGN KEY (created_by_user_id) REFERENCES users    (id) ON DELETE SET NULL
);

-- Case-insensitive uniqueness: the same alias string cannot map to two
-- separate entries (the Ambiguous path handles ≥2 employees sharing one alias
-- via separate rows; this index prevents exact-duplicate rows).
CREATE UNIQUE INDEX IF NOT EXISTS idx_employee_aliases_alias
    ON employee_aliases (lower(alias));

CREATE INDEX IF NOT EXISTS idx_employee_aliases_employee_id
    ON employee_aliases (employee_id);

-- ─── assignee_history ────────────────────────────────────────────────────────
-- Per-creator frequency table: how often each user assigns tasks to each
-- employee.  Used to boost confidence for "recently used" assignees so the
-- resolver surfaces the right person first for repeat creators.
-- Upsert pattern: INSERT … ON CONFLICT DO UPDATE (incrementing use_count and
-- refreshing last_used_at).
CREATE TABLE IF NOT EXISTS assignee_history (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    creator_user_id INTEGER NOT NULL,
    employee_id     INTEGER NOT NULL,
    last_used_at    TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    use_count       INTEGER NOT NULL DEFAULT 1,
    FOREIGN KEY (creator_user_id) REFERENCES users     (id) ON DELETE CASCADE,
    FOREIGN KEY (employee_id)     REFERENCES employees (id) ON DELETE CASCADE
);

-- Unique pair so we can do efficient upserts.
CREATE UNIQUE INDEX IF NOT EXISTS idx_assignee_history_pair
    ON assignee_history (creator_user_id, employee_id);

-- Supports "top-N most-used employees for this creator" query.
CREATE INDEX IF NOT EXISTS idx_assignee_history_creator_freq
    ON assignee_history (creator_user_id, use_count DESC, last_used_at DESC);

-- ─── pending_sheet_writes ────────────────────────────────────────────────────
-- Append-only write-back queue: rows are inserted when a user completes
-- onboarding via the bot (source = 'bot_registered') so the employee record
-- is reflected back into the Google Sheets directory.
--
-- The background SheetsWriteBackUseCase reads unwritten rows, calls the
-- Sheets API, and marks written_at on success.  On failure it increments
-- error_count; rows with error_count ≥ MAX_WRITE_BACK_ATTEMPTS are skipped.
CREATE TABLE IF NOT EXISTS pending_sheet_writes (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    employee_id        INTEGER NOT NULL,
    telegram_id        INTEGER NOT NULL,
    full_name          TEXT    NOT NULL,
    telegram_username  TEXT,
    created_at         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    written_at         TEXT,
    last_error         TEXT,
    error_count        INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY (employee_id) REFERENCES employees (id) ON DELETE CASCADE
);

-- One row per employee (dedup on the employee_id level).
CREATE UNIQUE INDEX IF NOT EXISTS idx_pending_sheet_writes_employee
    ON pending_sheet_writes (employee_id);

-- Supports the worker query: "give me pending rows with low error_count".
CREATE INDEX IF NOT EXISTS idx_pending_sheet_writes_pending
    ON pending_sheet_writes (written_at, error_count);

-- ─── Seed data: Russian common-name diminutives ───────────────────────────────
-- These aliases are seeded with employee_id = -1 (sentinel) because the actual
-- employee rows do not exist at migration time.  The alias resolver does a
-- JOIN on employees.full_name components to match; the runtime seed insertion
-- via SeedAliasesUseCase replaces these with real employee IDs at first sync.
-- We therefore leave the seed rows empty here and let the startup routine
-- populate them from known-good employee data.
-- (No data-seeding here to avoid referential integrity violations on the FK.)
