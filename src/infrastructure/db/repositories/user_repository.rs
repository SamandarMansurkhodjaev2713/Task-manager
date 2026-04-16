use sqlx::SqlitePool;

use crate::application::ports::repositories::UserRepository;
use crate::domain::errors::AppResult;
use crate::domain::user::User;
use crate::infrastructure::db::models::UserRow;

use super::common::{bool_as_i64, database_error, user_role_to_db, USER_COLUMNS};

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
            "INSERT INTO users (telegram_id, last_chat_id, telegram_username, full_name, is_employee, role, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(telegram_id) DO UPDATE SET
               last_chat_id = excluded.last_chat_id,
               telegram_username = excluded.telegram_username,
               full_name = excluded.full_name,
               is_employee = MAX(users.is_employee, excluded.is_employee),
               updated_at = excluded.updated_at
             RETURNING {USER_COLUMNS}"
        );

        let row = sqlx::query_as::<_, UserRow>(&query)
            .bind(user.telegram_id)
            .bind(user.last_chat_id)
            .bind(&user.telegram_username)
            .bind(&user.full_name)
            .bind(bool_as_i64(user.is_employee))
            .bind(user_role_to_db(user.role))
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
}
