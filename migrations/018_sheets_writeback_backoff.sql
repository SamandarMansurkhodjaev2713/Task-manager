-- Migration 018: add exponential-backoff column to pending_sheet_writes
--
-- F-09: Without a `next_attempt_at` gate the write-back worker retries every
-- flush interval regardless of how recently the last failure occurred.  This
-- means a transient Sheets API outage triggers up to MAX_FLUSH_BATCH_SIZE
-- requests per 5-minute tick, burning quota and spamming logs.
--
-- The new column stores the UTC timestamp before which the row must NOT be
-- picked up.  The write-back use case computes exponential backoff:
--   delay = min(2^error_count minutes, 240 minutes)
-- and sets next_attempt_at = now + delay on each failure.
--
-- Rollback: ALTER TABLE has no ROLLBACK in SQLite; a full table-rebuild would
-- be needed.  For the purposes of this single-instance deployment this is
-- considered acceptable.

ALTER TABLE pending_sheet_writes
    ADD COLUMN next_attempt_at TEXT;

-- Update the pending index to also cover the new column so the worker query
-- (written_at IS NULL AND error_count < N AND next_attempt_at <= now) is fast.
-- SQLite does not support ALTER INDEX, so we replace the old index by dropping
-- and recreating.
DROP INDEX IF EXISTS idx_pending_sheet_writes_pending;

CREATE INDEX IF NOT EXISTS idx_pending_sheet_writes_pending
    ON pending_sheet_writes (written_at, error_count, next_attempt_at);
