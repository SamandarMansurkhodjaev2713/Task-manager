-- 009_person_trigrams_and_idempotency.sql
--
-- Phase 1 — fuzzy assignee search index + generic idempotency ledger.
--
-- Rationale:
-- * `person_trigrams` holds normalised trigrams for employees AND users
--   (registered colleagues).  Rows are rebuilt by a background job from
--   `PersonName::trigrams`, and lookup is an `INNER JOIN GROUP BY owner`
--   scan — good enough for a ~30-person directory and avoids pulling an
--   extension like FTS5.  We scope each trigram to an `owner_kind`
--   (employee|user) + `owner_id` so deletions cascade cleanly when a user
--   is merged or an employee is removed from the directory.
-- * `idempotency_keys` is the canonical write-path dedupe ledger used by
--   use-cases like `CreateTaskFromMessage` for double-submit protection.
--   Rows carry `use_case`, `key`, and an optional `result_payload` so that
--   retries return the same response (not a "created" + a "duplicate").
--
-- SAFETY: new tables only — no existing data touched.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS person_trigrams (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    owner_kind TEXT NOT NULL CHECK (owner_kind IN ('employee', 'user')),
    owner_id INTEGER NOT NULL,
    trigram TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_person_trigrams_trigram ON person_trigrams (trigram);
CREATE INDEX IF NOT EXISTS idx_person_trigrams_owner
    ON person_trigrams (owner_kind, owner_id);
CREATE UNIQUE INDEX IF NOT EXISTS ux_person_trigrams_owner_trigram
    ON person_trigrams (owner_kind, owner_id, trigram);

CREATE TABLE IF NOT EXISTS idempotency_keys (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    use_case TEXT NOT NULL,
    key TEXT NOT NULL,
    result_payload TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    UNIQUE (use_case, key)
);

CREATE INDEX IF NOT EXISTS idx_idempotency_keys_created_at
    ON idempotency_keys (created_at);
