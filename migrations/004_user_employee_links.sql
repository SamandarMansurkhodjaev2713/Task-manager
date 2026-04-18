PRAGMA foreign_keys = ON;

ALTER TABLE users ADD COLUMN linked_employee_id INTEGER REFERENCES employees(id);

CREATE INDEX IF NOT EXISTS idx_users_linked_employee_id
    ON users (linked_employee_id);
