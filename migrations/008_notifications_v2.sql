-- 008_notifications_v2.sql
--
-- Phase 1 — Notification grouping + prioritisation fields used by
-- `DigestBuilder` in future phases.
--
-- Rationale:
-- * `group_key` lets us collapse several notifications into a digest
--   ("вы получили 3 обновления по задаче #1234") without changing the
--   wire format of the row itself.
-- * `priority` is a small integer (lower = higher priority).  Centralising
--   it here keeps the notification sender pipeline uniform across
--   `NotificationType`s.
--
-- Safety: additive; defaults ensure that pre-existing rows get a sensible
-- value (priority 100 = normal) and can be digested as a single-item group
-- (`group_key = NULL` => "do not group").

PRAGMA foreign_keys = ON;

ALTER TABLE notifications ADD COLUMN group_key TEXT;
ALTER TABLE notifications ADD COLUMN priority INTEGER NOT NULL DEFAULT 100;

CREATE INDEX IF NOT EXISTS idx_notifications_group_key
    ON notifications (group_key, created_at);
