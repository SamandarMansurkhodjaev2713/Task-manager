-- 007_admin_security_audit.sql
--
-- Phase 1 — RBAC + admin panel support tables.
--
-- Rationale:
-- * `admin_audit_log` captures *intentional* state changes performed by
--   admins (role promotion, user deactivation, feature flag toggles).  We
--   index on `created_at DESC` so that the admin panel can keyset-paginate
--   recent entries efficiently.
-- * `security_audit_log` captures *attempted* / denied events.  We keep
--   these separate so that forensic noise does not clutter the admin log
--   and retention policy can differ.
-- * `feature_flag_overrides` persists flags toggled at runtime from the
--   admin panel.  Enum values are open-ended text — `FeatureFlag::from_str`
--   skips unknown keys with a warning (so operators can add flags in new
--   releases without refusing to start).
-- * `admin_action_nonces` backs `AdminNonceStore`.  We prefer a DB-backed
--   store (not in-memory only) so that confirmations survive rollouts
--   where the bot briefly restarts between the admin clicking "change
--   role" and the subsequent "confirm" callback.
--
-- SAFETY:
-- * Pure additive migration — no existing table touched.
-- * Every actor/target id is a nullable INTEGER so that the audit log can
--   survive user row deletions in edge cases.

PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS admin_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_user_id INTEGER,
    target_user_id INTEGER,
    action_code TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (actor_user_id) REFERENCES users (id),
    FOREIGN KEY (target_user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_admin_audit_log_created_at
    ON admin_audit_log (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_admin_audit_log_target_user_id
    ON admin_audit_log (target_user_id);

CREATE TABLE IF NOT EXISTS security_audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    actor_user_id INTEGER,
    telegram_id INTEGER,
    event_code TEXT NOT NULL,
    metadata TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (actor_user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_security_audit_log_created_at
    ON security_audit_log (created_at DESC);
CREATE INDEX IF NOT EXISTS idx_security_audit_log_telegram_id
    ON security_audit_log (telegram_id);

CREATE TABLE IF NOT EXISTS feature_flag_overrides (
    flag_key TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL,
    updated_by_user_id INTEGER,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (updated_by_user_id) REFERENCES users (id)
);

CREATE TABLE IF NOT EXISTS admin_action_nonces (
    nonce TEXT PRIMARY KEY,
    actor_user_id INTEGER NOT NULL,
    purpose TEXT NOT NULL,
    payload TEXT NOT NULL DEFAULT '{}',
    expires_at TEXT NOT NULL,
    consumed_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (actor_user_id) REFERENCES users (id)
);

CREATE INDEX IF NOT EXISTS idx_admin_action_nonces_expires_at
    ON admin_action_nonces (expires_at);
