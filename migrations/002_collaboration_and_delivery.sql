PRAGMA foreign_keys = ON;

ALTER TABLE tasks ADD COLUMN version INTEGER NOT NULL DEFAULT 0;
ALTER TABLE tasks ADD COLUMN blocked_reason TEXT;
ALTER TABLE tasks ADD COLUMN blocked_at TEXT;
ALTER TABLE tasks ADD COLUMN review_requested_at TEXT;

ALTER TABLE notifications ADD COLUMN delivery_state TEXT NOT NULL DEFAULT 'pending';
ALTER TABLE notifications ADD COLUMN next_attempt_at TEXT;
ALTER TABLE notifications ADD COLUMN last_error_code TEXT;

ALTER TABLE comments ADD COLUMN kind TEXT NOT NULL DEFAULT 'context';

CREATE INDEX IF NOT EXISTS idx_tasks_status_deadline ON tasks (status, deadline);
CREATE INDEX IF NOT EXISTS idx_notifications_task_recipient_type
    ON notifications (task_id, recipient_user_id, notification_type, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_comments_task_created_at
    ON comments (task_id, created_at DESC);
