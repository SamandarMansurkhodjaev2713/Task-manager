-- 010_voice_processing.sql
--
-- Phase 1 — voice v2 idempotency + observability backing table.
--
-- Rationale:
-- * Telegram supplies `file_unique_id` for every voice message.  We use it
--   as the idempotency key so that replayed updates (network flaps,
--   duplicate webhook deliveries) do not trigger a second transcription
--   charge against OpenAI.  The UNIQUE index is the enforcement point.
-- * State machine is modelled as a string (`queued` / `transcribing` /
--   `transcribed` / `failed`) to keep the table simple and avoid schema
--   changes if we add new states in the future.
-- * `attempt_count` + `last_error_code` let us expose voice failures in
--   the Profile / admin dashboard without rehashing logs.
--
-- SAFETY: new table only; no existing schema touched.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS voice_processing_records (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_unique_id TEXT NOT NULL UNIQUE,
    chat_id INTEGER NOT NULL,
    telegram_user_id INTEGER NOT NULL,
    user_id INTEGER,
    state TEXT NOT NULL DEFAULT 'queued',
    attempt_count INTEGER NOT NULL DEFAULT 0,
    last_error_code TEXT,
    transcript_preview_hash TEXT,
    completed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_voice_processing_state_created_at
    ON voice_processing_records (state, created_at);
CREATE INDEX IF NOT EXISTS idx_voice_processing_user_id
    ON voice_processing_records (user_id);
