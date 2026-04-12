PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    telegram_id INTEGER NOT NULL UNIQUE,
    last_chat_id INTEGER,
    telegram_username TEXT,
    full_name TEXT,
    is_employee INTEGER NOT NULL DEFAULT 0,
    role TEXT NOT NULL DEFAULT 'user',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_users_telegram_id ON users (telegram_id);
CREATE INDEX IF NOT EXISTS idx_users_telegram_username ON users (telegram_username);

CREATE TABLE IF NOT EXISTS employees (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    full_name TEXT NOT NULL UNIQUE,
    telegram_username TEXT,
    email TEXT,
    phone TEXT,
    department TEXT,
    is_active INTEGER NOT NULL DEFAULT 1,
    synced_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_employees_name ON employees (full_name);
CREATE INDEX IF NOT EXISTS idx_employees_username ON employees (telegram_username);

CREATE TABLE IF NOT EXISTS tasks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_uid TEXT NOT NULL UNIQUE,
    source_message_key TEXT NOT NULL UNIQUE,
    created_by_user_id INTEGER NOT NULL,
    assigned_to_user_id INTEGER,
    assigned_to_employee_id INTEGER,
    title TEXT NOT NULL,
    description TEXT NOT NULL,
    acceptance_criteria TEXT NOT NULL DEFAULT '[]',
    expected_result TEXT NOT NULL,
    deadline TEXT,
    deadline_raw TEXT,
    original_message TEXT NOT NULL,
    message_type TEXT NOT NULL,
    ai_model_used TEXT NOT NULL,
    ai_response_raw TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'created',
    priority TEXT NOT NULL DEFAULT 'medium',
    telegram_chat_id INTEGER NOT NULL,
    telegram_message_id INTEGER NOT NULL,
    telegram_task_message_id INTEGER,
    tags TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    sent_at TEXT,
    started_at TEXT,
    completed_at TEXT,
    cancelled_at TEXT,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (created_by_user_id) REFERENCES users (id),
    FOREIGN KEY (assigned_to_user_id) REFERENCES users (id),
    FOREIGN KEY (assigned_to_employee_id) REFERENCES employees (id)
);

CREATE INDEX IF NOT EXISTS idx_tasks_created_by_user_id ON tasks (created_by_user_id);
CREATE INDEX IF NOT EXISTS idx_tasks_assigned_to_user_id ON tasks (assigned_to_user_id);
CREATE INDEX IF NOT EXISTS idx_tasks_deadline ON tasks (deadline);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON tasks (status);

CREATE TABLE IF NOT EXISTS task_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    action TEXT NOT NULL,
    old_status TEXT,
    new_status TEXT,
    changed_by_user_id INTEGER,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (task_id) REFERENCES tasks (id),
    FOREIGN KEY (changed_by_user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_task_history_task_id ON task_history (task_id);
CREATE INDEX IF NOT EXISTS idx_task_history_created_at ON task_history (created_at);

CREATE TABLE IF NOT EXISTS notifications (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER,
    recipient_user_id INTEGER NOT NULL,
    notification_type TEXT NOT NULL,
    message TEXT NOT NULL,
    dedupe_key TEXT NOT NULL UNIQUE,
    telegram_message_id INTEGER,
    is_sent INTEGER NOT NULL DEFAULT 0,
    is_read INTEGER NOT NULL DEFAULT 0,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    sent_at TEXT,
    read_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (task_id) REFERENCES tasks (id),
    FOREIGN KEY (recipient_user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_notifications_pending
    ON notifications (is_sent, recipient_user_id, created_at);

CREATE TABLE IF NOT EXISTS comments (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    task_id INTEGER NOT NULL,
    author_user_id INTEGER NOT NULL,
    body TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (task_id) REFERENCES tasks (id),
    FOREIGN KEY (author_user_id) REFERENCES users (id)
);

CREATE TABLE IF NOT EXISTS app_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    level TEXT NOT NULL,
    module TEXT,
    message TEXT NOT NULL,
    context TEXT NOT NULL DEFAULT '{}',
    error_trace TEXT,
    user_id INTEGER,
    task_id INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users (id),
    FOREIGN KEY (task_id) REFERENCES tasks (id)
);
