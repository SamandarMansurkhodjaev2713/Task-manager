-- 017_voice_transcript_storage.sql
--
-- Phase 1.2 (audit fix F-01) — wire VoiceProcessingRepository into the
-- voice creation flow for true Whisper-call idempotency.
--
-- Adds a column for caching the actual transcript text so a duplicate
-- webhook delivery (same Telegram `file_unique_id`) can return the
-- previously transcribed result instead of either:
--   * silently re-charging OpenAI for the same audio, or
--   * losing the user's work when the bot restarts mid-flow.
--
-- The transcript is purged together with the rest of the row's payload
-- by the existing `purge_stale_payloads` job (governed by
-- `VOICE_PROCESSING_RETENTION_MINUTES`).  The retention window is short
-- (default 60 minutes) so user content is not held in the database any
-- longer than necessary for end-of-flow retries.
--
-- SAFETY: additive migration, no existing rows touched.

PRAGMA foreign_keys = ON;

ALTER TABLE voice_processing_records ADD COLUMN transcript_text TEXT;
