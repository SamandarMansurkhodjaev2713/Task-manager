-- 014_drop_app_logs.sql
--
-- The `app_logs` table was created in migration 001 as a catch-all structured
-- log sink, but no write path was ever wired up (zero rows in production).
-- All structured events are now routed to either `admin_audit_log` (phase 4),
-- `security_audit_log` (phase 4), or the `tracing` subscriber (stdout/file).
-- Keeping an empty, un-indexed table in the hot WAL path is pure overhead.
--
-- SAFETY: additive removal.  No FK references to this table exist anywhere in
-- the schema; the DROP is unconditional but wrapped in IF EXISTS so re-running
-- migrations on a DB that never had migration 001 is still safe.

PRAGMA foreign_keys = ON;

DROP TABLE IF EXISTS app_logs;
