use chrono::{DateTime, Utc};
use sqlx::{Acquire, SqlitePool};

use crate::application::ports::repositories::{BootstrapPromotion, UserRepository};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::user::{OnboardingState, User, UserRole};
use crate::infrastructure::db::models::UserRow;

use super::common::{bool_as_i64, database_error, user_role_to_db, USER_COLUMNS};

const ONBOARDING_CONCURRENCY_CONFLICT: &str = "USER_ONBOARDING_VERSION_CONFLICT";
/// Error code returned when demoting / deactivating the last remaining
/// active admin would leave the system without any administrative
/// authority.  Bootstrap recovery is still possible via the `.env`-driven
/// `AdminIdSet`, but that requires a restart, so we prefer to refuse the
/// operation at the repository layer and surface a human-readable error.
pub const LAST_ADMIN_PROTECTED: &str = "LAST_ADMIN_PROTECTED";
/// Error code returned when the target user could not be found during a
/// role change or (de)activation attempt.
pub const USER_NOT_FOUND: &str = "USER_NOT_FOUND";

#[derive(Clone)]
pub struct SqliteUserRepository {
    pool: SqlitePool,
}

impl SqliteUserRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl UserRepository for SqliteUserRepository {
    async fn upsert_from_message(&self, user: &User) -> AppResult<User> {
        let query = format!(
            "INSERT INTO users (telegram_id, last_chat_id, telegram_username, full_name, first_name, last_name, linked_employee_id, is_employee, role, onboarding_state, onboarding_version, timezone, quiet_hours_start_min, quiet_hours_end_min, deactivated_at, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(telegram_id) DO UPDATE SET
               last_chat_id = excluded.last_chat_id,
               telegram_username = excluded.telegram_username,
               full_name = excluded.full_name,
               first_name = COALESCE(excluded.first_name, users.first_name),
               last_name = COALESCE(excluded.last_name, users.last_name),
               linked_employee_id = COALESCE(excluded.linked_employee_id, users.linked_employee_id),
               is_employee = MAX(users.is_employee, excluded.is_employee),
               updated_at = excluded.updated_at
             RETURNING {USER_COLUMNS}"
        );

        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(user.telegram_id)
            .bind(user.last_chat_id)
            .bind(&user.telegram_username)
            .bind(&user.full_name)
            .bind(&user.first_name)
            .bind(&user.last_name)
            .bind(user.linked_employee_id)
            .bind(bool_as_i64(user.is_employee))
            .bind(user_role_to_db(user.role))
            .bind(user.onboarding_state.as_storage_value())
            .bind(user.onboarding_version)
            .bind(&user.timezone)
            .bind(user.quiet_hours_start_min as i64)
            .bind(user.quiet_hours_end_min as i64)
            .bind(user.deactivated_at)
            .bind(user.created_at)
            .bind(user.updated_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn find_by_id(&self, user_id: i64) -> AppResult<Option<User>> {
        let query = format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?");
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn find_by_telegram_id(&self, telegram_id: i64) -> AppResult<Option<User>> {
        let query = format!("SELECT {USER_COLUMNS} FROM users WHERE telegram_id = ?");
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(telegram_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn find_by_username(&self, username: &str) -> AppResult<Option<User>> {
        let query =
            format!("SELECT {USER_COLUMNS} FROM users WHERE lower(telegram_username) = lower(?)");
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(username.trim_start_matches('@'))
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;
        row.map(TryInto::try_into).transpose()
    }

    async fn list_with_chat_id(&self) -> AppResult<Vec<User>> {
        let query = format!(
            "SELECT {USER_COLUMNS} FROM users WHERE last_chat_id IS NOT NULL ORDER BY id ASC"
        );
        let rows = sqlx::query_as::<_, UserRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn ensure_onboarding_session(
        &self,
        telegram_id: i64,
        chat_id: i64,
        telegram_username: Option<&str>,
        fallback_full_name: Option<&str>,
        now: DateTime<Utc>,
    ) -> AppResult<User> {
        // If an onboarding-in-progress or freshly-onboarded row exists, just
        // update volatile fields (chat_id, username) and return.  Otherwise
        // create a new row with `onboarding_state=awaiting_first_name`.
        let existing = self.find_by_telegram_id(telegram_id).await?;
        match existing {
            Some(user) => {
                let query = format!(
                    "UPDATE users
                     SET last_chat_id = ?,
                         telegram_username = ?,
                         updated_at = ?
                     WHERE id = ?
                     RETURNING {USER_COLUMNS}"
                );
                let row = sqlx::query_as::<_, UserRow>(&query)
                    .bind(chat_id)
                    .bind(telegram_username)
                    .bind(now)
                    .bind(user.id.expect("persisted user has id"))
                    .fetch_one(&self.pool)
                    .await
                    .map_err(database_error)?;
                row.try_into()
            }
            None => {
                let query = format!(
                    "INSERT INTO users (telegram_id, last_chat_id, telegram_username, full_name, onboarding_state, onboarding_version, created_at, updated_at)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?)
                     RETURNING {USER_COLUMNS}"
                );
                let row = sqlx::query_as::<_, UserRow>(&query)
                    .bind(telegram_id)
                    .bind(chat_id)
                    .bind(telegram_username)
                    .bind(fallback_full_name)
                    .bind(OnboardingState::AwaitingFirstName.as_storage_value())
                    .bind(0_i64)
                    .bind(now)
                    .bind(now)
                    .fetch_one(&self.pool)
                    .await
                    .map_err(database_error)?;
                row.try_into()
            }
        }
    }

    async fn save_onboarding_progress(
        &self,
        user_id: i64,
        expected_version: i64,
        next_state: OnboardingState,
        first_name: Option<&str>,
        last_name: Option<&str>,
        now: DateTime<Utc>,
    ) -> AppResult<User> {
        let query = format!(
            "UPDATE users
             SET first_name = COALESCE(?, first_name),
                 last_name = COALESCE(?, last_name),
                 onboarding_state = ?,
                 onboarding_version = onboarding_version + 1,
                 updated_at = ?
             WHERE id = ? AND onboarding_version = ?
             RETURNING {USER_COLUMNS}"
        );
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(first_name)
            .bind(last_name)
            .bind(next_state.as_storage_value())
            .bind(now)
            .bind(user_id)
            .bind(expected_version)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;

        match row {
            Some(row) => row.try_into(),
            None => Err(onboarding_conflict_error(user_id, expected_version)),
        }
    }

    async fn complete_onboarding(
        &self,
        user_id: i64,
        expected_version: i64,
        first_name: &str,
        last_name: &str,
        linked_employee_id: Option<i64>,
        now: DateTime<Utc>,
    ) -> AppResult<User> {
        let canonical_full = format!("{first_name} {last_name}");
        let query = format!(
            "UPDATE users
             SET first_name = ?,
                 last_name = ?,
                 full_name = ?,
                 linked_employee_id = COALESCE(?, linked_employee_id),
                 is_employee = MAX(is_employee, CASE WHEN ? IS NULL THEN 0 ELSE 1 END),
                 onboarding_state = ?,
                 onboarding_version = onboarding_version + 1,
                 updated_at = ?
             WHERE id = ? AND onboarding_version = ?
             RETURNING {USER_COLUMNS}"
        );
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(first_name)
            .bind(last_name)
            .bind(&canonical_full)
            .bind(linked_employee_id)
            .bind(linked_employee_id)
            .bind(OnboardingState::Completed.as_storage_value())
            .bind(now)
            .bind(user_id)
            .bind(expected_version)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?;

        match row {
            Some(row) => row.try_into(),
            None => Err(onboarding_conflict_error(user_id, expected_version)),
        }
    }

    async fn set_role(
        &self,
        _actor_user_id: i64,
        target_user_id: i64,
        new_role: UserRole,
        now: DateTime<Utc>,
    ) -> AppResult<User> {
        // Use an immediate transaction so the admin-count check and the role
        // update happen atomically.  Without this, two parallel demotions
        // could each see "two admins remain" and both succeed, dropping the
        // count below 1.
        let mut conn = self.pool.acquire().await.map_err(database_error)?;
        let mut tx = conn.begin().await.map_err(database_error)?;

        let target = load_user_by_id_in_tx(&mut tx, target_user_id).await?;
        let previous_role = target.role;

        if previous_role == UserRole::Admin
            && new_role != UserRole::Admin
            && target.deactivated_at.is_none()
            && count_other_active_admins_in_tx(&mut tx, target_user_id).await? == 0
        {
            return Err(last_admin_protected_error(
                "set_role",
                target_user_id,
                previous_role,
                Some(new_role),
            ));
        }

        let query = format!(
            "UPDATE users
             SET role = ?, updated_at = ?
             WHERE id = ?
             RETURNING {USER_COLUMNS}"
        );
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(user_role_to_db(new_role))
            .bind(now)
            .bind(target_user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(database_error)?;

        tx.commit().await.map_err(database_error)?;
        row.try_into()
    }

    async fn deactivate(
        &self,
        _actor_user_id: i64,
        target_user_id: i64,
        now: DateTime<Utc>,
    ) -> AppResult<User> {
        let mut conn = self.pool.acquire().await.map_err(database_error)?;
        let mut tx = conn.begin().await.map_err(database_error)?;

        let target = load_user_by_id_in_tx(&mut tx, target_user_id).await?;

        if target.role == UserRole::Admin
            && target.deactivated_at.is_none()
            && count_other_active_admins_in_tx(&mut tx, target_user_id).await? == 0
        {
            return Err(last_admin_protected_error(
                "deactivate",
                target_user_id,
                target.role,
                None,
            ));
        }

        let query = format!(
            "UPDATE users
             SET deactivated_at = ?, updated_at = ?
             WHERE id = ?
             RETURNING {USER_COLUMNS}"
        );
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(now)
            .bind(now)
            .bind(target_user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(database_error)?;

        tx.commit().await.map_err(database_error)?;
        row.try_into()
    }

    async fn reactivate(
        &self,
        _actor_user_id: i64,
        target_user_id: i64,
        now: DateTime<Utc>,
    ) -> AppResult<User> {
        let query = format!(
            "UPDATE users
             SET deactivated_at = NULL, updated_at = ?
             WHERE id = ?
             RETURNING {USER_COLUMNS}"
        );
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(now)
            .bind(target_user_id)
            .fetch_optional(&self.pool)
            .await
            .map_err(database_error)?
            .ok_or_else(|| user_not_found_error(target_user_id))?;
        row.try_into()
    }

    async fn list_active_admins(&self) -> AppResult<Vec<User>> {
        let query = format!(
            "SELECT {USER_COLUMNS}
             FROM users
             WHERE role = 'admin' AND deactivated_at IS NULL
             ORDER BY id ASC"
        );
        let rows = sqlx::query_as::<_, UserRow>(&query)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }

    async fn promote_bootstrap_admin(
        &self,
        telegram_id: i64,
        now: DateTime<Utc>,
    ) -> AppResult<Option<BootstrapPromotion>> {
        // Bootstrap only promotes *existing* users, never silently creates
        // them — this keeps the `.env`-driven admin list side-effect-free
        // until the operator performs their first `/start` and the row is
        // hydrated with real Telegram metadata.
        let existing = self.find_by_telegram_id(telegram_id).await?;
        let Some(user) = existing else {
            return Ok(None);
        };
        if user.role == UserRole::Admin {
            return Ok(Some(BootstrapPromotion::AlreadyAdmin(user)));
        }

        let query = format!(
            "UPDATE users
             SET role = 'admin', deactivated_at = NULL, updated_at = ?
             WHERE id = ?
             RETURNING {USER_COLUMNS}"
        );
        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(now)
            .bind(user.id.expect("persisted user has id"))
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        let elevated: User = row.try_into()?;
        Ok(Some(BootstrapPromotion::Elevated(elevated)))
    }
}

async fn load_user_by_id_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    user_id: i64,
) -> AppResult<User> {
    let query = format!("SELECT {USER_COLUMNS} FROM users WHERE id = ?");
    let row = sqlx::query_as::<_, UserRow>(&query)
        .bind(user_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(database_error)?
        .ok_or_else(|| user_not_found_error(user_id))?;
    row.try_into()
}

async fn count_other_active_admins_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    excluding_user_id: i64,
) -> AppResult<i64> {
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM users
         WHERE role = 'admin' AND deactivated_at IS NULL AND id <> ?",
    )
    .bind(excluding_user_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(database_error)?;
    Ok(count)
}

fn last_admin_protected_error(
    operation: &'static str,
    target_user_id: i64,
    current_role: UserRole,
    requested_role: Option<UserRole>,
) -> AppError {
    AppError::business_rule(
        LAST_ADMIN_PROTECTED,
        "Cannot remove the last active administrator; promote another admin first",
        serde_json::json!({
            "operation": operation,
            "target_user_id": target_user_id,
            "current_role": current_role.to_string(),
            "requested_role": requested_role.map(|role| role.to_string()),
        }),
    )
}

fn user_not_found_error(user_id: i64) -> AppError {
    AppError::not_found(
        USER_NOT_FOUND,
        "Target user does not exist",
        serde_json::json!({ "user_id": user_id }),
    )
}

fn onboarding_conflict_error(user_id: i64, expected_version: i64) -> AppError {
    AppError::business_rule(
        ONBOARDING_CONCURRENCY_CONFLICT,
        "Onboarding state changed concurrently; restart the step",
        serde_json::json!({
            "user_id": user_id,
            "expected_version": expected_version,
        }),
    )
}
