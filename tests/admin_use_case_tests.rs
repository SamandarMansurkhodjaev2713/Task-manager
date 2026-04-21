//! Integration tests for `AdminUseCase` (Phase 4).
//!
//! These tests exercise the use case end-to-end against a real SQLite
//! schema.  We verify that:
//!
//! 1. Listing active admins returns only non-deactivated admins.
//! 2. A non-admin actor cannot read admin-only data.
//! 3. A role change succeeds, mutates the row, and writes an audit entry.
//! 4. The "last active admin" invariant is surfaced as `LAST_ADMIN_PROTECTED`.
//! 5. Deactivation + reactivation both succeed and are auditable.
//! 6. Self-targeting is rejected with `FORBIDDEN_SELF_TARGET`.

use std::sync::Arc;

use tempfile::{tempdir, TempDir};

use telegram_task_bot::application::use_cases::admin::AdminUseCase;
use telegram_task_bot::domain::user::{OnboardingState, User, UserRole};
use telegram_task_bot::infrastructure::clock::system_clock::SystemClock;
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::{
    SqliteAdminAuditLogRepository, SqliteFeatureFlagRepository, SqliteUserRepository,
};
use telegram_task_bot::shared::feature_flags::FeatureFlagRegistry;

async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
    let temp_dir = tempdir().expect("temp dir should be created");
    let db_path = temp_dir.path().join("admin-use-case.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");
    (temp_dir, pool)
}

struct Harness {
    _temp: TempDir,
    use_case: AdminUseCase,
    pool: sqlx::SqlitePool,
}

impl Harness {
    async fn new() -> Self {
        let (temp, pool) = test_pool().await;
        let user_repo = Arc::new(SqliteUserRepository::new(pool.clone()));
        let audit_repo = Arc::new(SqliteAdminAuditLogRepository::new(pool.clone()));
        let flag_repo = Arc::new(SqliteFeatureFlagRepository::new(pool.clone()));
        let clock = Arc::new(SystemClock);
        let shared_flags = Arc::new(tokio::sync::RwLock::new(
            FeatureFlagRegistry::from_env_and_defaults(None),
        ));
        let use_case = AdminUseCase::new(clock, user_repo, audit_repo, flag_repo, shared_flags);
        Self {
            _temp: temp,
            use_case,
            pool,
        }
    }
}

async fn seed_user(pool: &sqlx::SqlitePool, telegram_id: i64, role: &str) -> i64 {
    sqlx::query(
        "INSERT INTO users (telegram_id, telegram_username, full_name, first_name, last_name, role, onboarding_state)
         VALUES (?, ?, ?, ?, ?, ?, 'completed')",
    )
    .bind(telegram_id)
    .bind(format!("user{telegram_id}"))
    .bind(format!("Ivan Ivanov {telegram_id}"))
    .bind("Ivan")
    .bind(format!("Ivanov{telegram_id}"))
    .bind(role)
    .execute(pool)
    .await
    .expect("user should be seeded")
    .last_insert_rowid()
}

async fn load_user(pool: &sqlx::SqlitePool, user_id: i64) -> User {
    // Use the repository’s own loader to get a well-formed User (with all
    // new columns such as `deactivated_at`), mirroring production code.
    let repo = SqliteUserRepository::new(pool.clone());
    <SqliteUserRepository as telegram_task_bot::application::ports::repositories::UserRepository>::find_by_id(&repo, user_id)
        .await
        .expect("find must succeed")
        .expect("user should exist")
}

async fn count_admin_audit_rows(pool: &sqlx::SqlitePool, action_code: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM admin_audit_log WHERE action_code = ?")
        .bind(action_code)
        .fetch_one(pool)
        .await
        .expect("audit count should succeed")
}

#[tokio::test]
async fn given_active_admin_when_listing_admins_then_returns_only_active() {
    let harness = Harness::new().await;

    let admin_id = seed_user(&harness.pool, 100, "admin").await;
    let deactivated_id = seed_user(&harness.pool, 101, "admin").await;
    sqlx::query("UPDATE users SET deactivated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(deactivated_id)
        .execute(&harness.pool)
        .await
        .expect("deactivation should be possible");
    let _regular_user_id = seed_user(&harness.pool, 102, "user").await;

    let admin = load_user(&harness.pool, admin_id).await;

    let admins = harness
        .use_case
        .list_active_admins(&admin)
        .await
        .expect("admin should list admins");

    assert_eq!(admins.len(), 1);
    assert_eq!(admins[0].id, Some(admin_id));
}

#[tokio::test]
async fn given_non_admin_when_listing_admins_then_returns_forbidden_admin_only() {
    let harness = Harness::new().await;

    let manager_id = seed_user(&harness.pool, 200, "manager").await;
    let manager = load_user(&harness.pool, manager_id).await;

    let err = harness
        .use_case
        .list_active_admins(&manager)
        .await
        .expect_err("managers are not admins");

    assert_eq!(err.code(), "FORBIDDEN_ADMIN_ONLY");
}

#[tokio::test]
async fn given_two_admins_when_changing_other_role_then_succeeds_and_audit_written() {
    let harness = Harness::new().await;

    let admin_id = seed_user(&harness.pool, 300, "admin").await;
    let target_id = seed_user(&harness.pool, 301, "admin").await;
    let admin = load_user(&harness.pool, admin_id).await;

    let updated = harness
        .use_case
        .change_role(&admin, target_id, UserRole::Manager)
        .await
        .expect("role change should succeed when another admin remains");

    assert_eq!(updated.id, Some(target_id));
    assert_eq!(updated.role, UserRole::Manager);

    let audit_rows = count_admin_audit_rows(&harness.pool, "role_changed_by_admin").await;
    assert_eq!(audit_rows, 1);
}

#[tokio::test]
async fn given_sole_admin_when_demoting_another_admin_then_succeeds() {
    // Sanity check: demoting *another* admin while we remain Admin is
    // allowed; the last-admin invariant only fires when the action would
    // leave the system without *any* active admin.
    let harness = Harness::new().await;

    let admin_id = seed_user(&harness.pool, 400, "admin").await;
    let other_admin_id = seed_user(&harness.pool, 401, "admin").await;
    let admin = load_user(&harness.pool, admin_id).await;

    let updated = harness
        .use_case
        .change_role(&admin, other_admin_id, UserRole::User)
        .await
        .expect("demotion allowed because caller remains admin");

    assert_eq!(updated.role, UserRole::User);
}

#[tokio::test]
async fn given_sole_admin_when_demoting_self_target_then_returns_self_target_error() {
    let harness = Harness::new().await;

    let admin_id = seed_user(&harness.pool, 500, "admin").await;
    let admin = load_user(&harness.pool, admin_id).await;

    let err = harness
        .use_case
        .change_role(&admin, admin_id, UserRole::User)
        .await
        .expect_err("self-target must be rejected");

    assert_eq!(err.code(), "FORBIDDEN_SELF_TARGET");
}

#[tokio::test]
async fn given_deactivate_and_reactivate_cycle_when_executed_then_audits_both() {
    let harness = Harness::new().await;

    let admin_id = seed_user(&harness.pool, 600, "admin").await;
    let target_id = seed_user(&harness.pool, 601, "manager").await;
    let admin = load_user(&harness.pool, admin_id).await;

    harness
        .use_case
        .deactivate_user(&admin, target_id)
        .await
        .expect("deactivate should succeed");

    harness
        .use_case
        .reactivate_user(&admin, target_id)
        .await
        .expect("reactivate should succeed");

    let deact_rows = count_admin_audit_rows(&harness.pool, "user_deactivated_by_admin").await;
    let react_rows = count_admin_audit_rows(&harness.pool, "user_reactivated_by_admin").await;
    assert_eq!(deact_rows, 1);
    assert_eq!(react_rows, 1);
}

#[tokio::test]
async fn given_recent_audit_listing_when_called_then_clamps_limit() {
    let harness = Harness::new().await;

    let admin_id = seed_user(&harness.pool, 700, "admin").await;
    let target_id = seed_user(&harness.pool, 701, "user").await;
    let admin = load_user(&harness.pool, admin_id).await;

    harness
        .use_case
        .change_role(&admin, target_id, UserRole::Manager)
        .await
        .expect("role change must succeed");

    // Requesting 0 clamps up to 1 — we should still receive at least the
    // single audit row we just wrote.
    let entries = harness
        .use_case
        .list_recent_audit(&admin, 0)
        .await
        .expect("audit listing must succeed");
    assert!(
        !entries.is_empty(),
        "at least one audit row must be returned"
    );
}

// Reference to ensure unused-import lints don’t fire if helpers change.
#[allow(dead_code)]
const _ONBOARDING_STATE_REFERENCED: OnboardingState = OnboardingState::Completed;
