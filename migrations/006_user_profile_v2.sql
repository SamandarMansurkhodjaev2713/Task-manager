-- 006_user_profile_v2.sql
--
-- Phase 1 — Onboarding v2 / Profile / Notifications preferences.
--
-- Rationale:
-- * We need structured first+last names (see PersonName value object).  We
--   do NOT drop `full_name` — `first_name` + `last_name` are populated
--   during onboarding and the old column remains as a legacy fallback so
--   that this migration is strictly additive and reversible via a plain
--   `UPDATE users SET first_name=NULL, last_name=NULL;`.
-- * `onboarding_state` captures the FSM step the user is currently at.
--   Nullable so that existing users stay in `'completed'` implicitly
--   (handled in code — the NULL=>completed mapping is set by the
--   OnboardingStateRepository read path).
-- * `onboarding_version` enables optimistic concurrency for the FSM
--   without introducing a separate `sessions` table.
-- * `timezone` defaults to Europe/Moscow — the fleet is RU-only for v3.
-- * Quiet hours default to 22:00..08:00 user-local.  Stored as integer
--   minutes-from-midnight for cheap comparison in SQL.
-- * `deactivated_at` enables soft-delete (an admin may deactivate a user
--   without orphaning tasks/comments).
--
-- SAFETY:
-- * All ALTERs use `ADD COLUMN` with NULL or an explicit DEFAULT so SQLite
--   does not need to rewrite the table.
-- * No existing column types are changed, no rows are deleted.

PRAGMA foreign_keys = ON;

ALTER TABLE users ADD COLUMN first_name TEXT;
ALTER TABLE users ADD COLUMN last_name TEXT;
ALTER TABLE users ADD COLUMN onboarding_state TEXT;
ALTER TABLE users ADD COLUMN onboarding_version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN timezone TEXT NOT NULL DEFAULT 'Europe/Moscow';
ALTER TABLE users ADD COLUMN quiet_hours_start_min INTEGER NOT NULL DEFAULT 1320;
ALTER TABLE users ADD COLUMN quiet_hours_end_min INTEGER NOT NULL DEFAULT 480;
ALTER TABLE users ADD COLUMN deactivated_at TEXT;

CREATE INDEX IF NOT EXISTS idx_users_onboarding_state ON users (onboarding_state);
CREATE INDEX IF NOT EXISTS idx_users_deactivated_at ON users (deactivated_at);
