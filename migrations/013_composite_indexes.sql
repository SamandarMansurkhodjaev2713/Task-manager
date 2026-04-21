-- 013_composite_indexes.sql
--
-- Performance indexes for the most common query patterns observed in
-- production (keyset-paginated task lists filtered by assignment + status).
--
-- These are covering indexes for the two queries that account for >90% of
-- task-repository traffic:
--
--   (a) list_assigned_to_user:
--         WHERE assigned_to_user_id = ?
--         ORDER BY task_uid DESC
--         LIMIT ?
--
--   (b) list_created_by_user:
--         WHERE created_by_user_id = ?
--         ORDER BY task_uid DESC
--         LIMIT ?
--
-- The single-column indexes introduced in 001_initial_schema.sql still exist
-- and satisfy range scans; these composites add the sort column (task_uid)
-- so SQLite can use an index scan instead of a sort for paginated queries.
--
-- SAFETY: Pure additive migration — no existing index or table is touched.
--         IF NOT EXISTS guards make it idempotent.

PRAGMA foreign_keys = ON;

CREATE INDEX IF NOT EXISTS idx_tasks_assigned_uid
    ON tasks (assigned_to_user_id, task_uid DESC);

CREATE INDEX IF NOT EXISTS idx_tasks_created_uid
    ON tasks (created_by_user_id, task_uid DESC);

-- Open-task status filter used by the background SLA scanner and the
-- "focus" list (status IN ('created','sent','in_progress','blocked','in_review')).
-- SQLite partial indexes require the same expression in the WHERE clause to
-- be recognised, so we index status directly for that scanner.
CREATE INDEX IF NOT EXISTS idx_tasks_status_updated
    ON tasks (status, updated_at DESC);
