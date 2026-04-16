PRAGMA foreign_keys = ON;

CREATE INDEX IF NOT EXISTS idx_tasks_employee_assignment_recovery
    ON tasks (assigned_to_employee_id, assigned_to_user_id, status, updated_at DESC);

CREATE INDEX IF NOT EXISTS idx_notifications_delivery_queue
    ON notifications (delivery_state, next_attempt_at, created_at ASC);
