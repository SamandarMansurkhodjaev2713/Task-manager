//! Integration tests for the RBAC "last-admin" invariant enforced at the
//! repository layer (see `SqliteUserRepository::set_role` / `deactivate`).
//!
//! Why these tests live in the integration suite: the invariant relies on
//! a transactional `SELECT COUNT(...) → UPDATE` round-trip against SQLite,
//! which we refuse to exercise with hand-rolled mocks.

use chrono::Utc;
use tempfile::{tempdir, TempDir};

use telegram_task_bot::application::ports::repositories::UserRepository;
use telegram_task_bot::domain::user::UserRole;
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::SqliteUserRepository;

async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
    let temp_dir = tempdir().expect("temp dir should be created");
    let db_path = temp_dir.path().join("rbac.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");
    (temp_dir, pool)
}

/// Seeds one admin row and returns its primary key.
async fn seed_admin(pool: &sqlx::SqlitePool, telegram_id: i64) -> i64 {
    sqlx::query(
        "INSERT INTO users (telegram_id, telegram_username, full_name, role)
         VALUES (?, ?, ?, 'admin')",
    )
    .bind(telegram_id)
    .bind(format!("admin{telegram_id}"))
    .bind(format!("Admin {telegram_id}"))
    .execute(pool)
    .await
    .expect("admin should be seeded")
    .last_insert_rowid()
}

#[tokio::test]
async fn given_sole_admin_when_demoting_then_returns_last_admin_protected_error() {
    let (_temp, pool) = test_pool().await;
    let user_repo = SqliteUserRepository::new(pool.clone());

    let admin_id = seed_admin(&pool, 100).await;
    let requester_id = admin_id; // requester is the admin themselves (for the purpose of the
                                 // *repository* check; the policy-layer self-target guard is
                                 // exercised separately)

    let err = user_repo
        .set_role(requester_id, admin_id, UserRole::User, Utc::now())
        .await
        .expect_err("sole admin cannot be demoted");

    assert_eq!(err.code(), "LAST_ADMIN_PROTECTED");
}

#[tokio::test]
async fn given_sole_admin_when_deactivating_then_returns_last_admin_protected_error() {
    let (_temp, pool) = test_pool().await;
    let user_repo = SqliteUserRepository::new(pool.clone());

    let admin_id = seed_admin(&pool, 200).await;

    let err = user_repo
        .deactivate(admin_id, admin_id, Utc::now())
        .await
        .expect_err("sole admin cannot be deactivated");

    assert_eq!(err.code(), "LAST_ADMIN_PROTECTED");
}

#[tokio::test]
async fn given_two_admins_when_demoting_one_then_succeeds_and_other_remains_admin() {
    let (_temp, pool) = test_pool().await;
    let user_repo = SqliteUserRepository::new(pool.clone());

    let first_admin = seed_admin(&pool, 300).await;
    let second_admin = seed_admin(&pool, 301).await;

    let updated = user_repo
        .set_role(second_admin, first_admin, UserRole::User, Utc::now())
        .await
        .expect("demotion allowed because another admin remains");

    assert_eq!(updated.role, UserRole::User);

    let admins = user_repo
        .list_active_admins()
        .await
        .expect("list should succeed");
    assert_eq!(admins.len(), 1);
    assert_eq!(admins[0].id, Some(second_admin));
}

#[tokio::test]
async fn given_sole_admin_but_already_deactivated_when_demoting_then_succeeds() {
    // An already-deactivated admin is NOT an "active admin" for invariant
    // purposes — demoting them is therefore allowed, since they don't
    // currently hold any authority.  This preserves the operator's ability
    // to tidy up legacy rows after a migration.
    let (_temp, pool) = test_pool().await;
    let user_repo = SqliteUserRepository::new(pool.clone());

    let admin_id = seed_admin(&pool, 400).await;
    // Deactivate via direct SQL to bypass the invariant path.
    sqlx::query("UPDATE users SET deactivated_at = CURRENT_TIMESTAMP WHERE id = ?")
        .bind(admin_id)
        .execute(&pool)
        .await
        .expect("deactivation should be possible via raw SQL");

    let updated = user_repo
        .set_role(admin_id, admin_id, UserRole::User, Utc::now())
        .await
        .expect("demotion allowed for already-deactivated admin");

    assert_eq!(updated.role, UserRole::User);
}

#[tokio::test]
async fn given_missing_user_when_setting_role_then_returns_user_not_found() {
    let (_temp, pool) = test_pool().await;
    let user_repo = SqliteUserRepository::new(pool);

    let err = user_repo
        .set_role(1, 9999, UserRole::Admin, Utc::now())
        .await
        .expect_err("non-existent user must fail");

    assert_eq!(err.code(), "USER_NOT_FOUND");
}
