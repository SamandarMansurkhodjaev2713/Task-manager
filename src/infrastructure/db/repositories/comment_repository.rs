use sqlx::SqlitePool;

use crate::application::ports::repositories::CommentRepository;
use crate::domain::comment::TaskComment;
use crate::domain::errors::AppResult;
use crate::infrastructure::db::models::CommentRow;

use super::common::{comment_kind_to_db, database_error, COMMENT_COLUMNS};

#[derive(Clone)]
pub struct SqliteCommentRepository {
    pool: SqlitePool,
}

impl SqliteCommentRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl CommentRepository for SqliteCommentRepository {
    async fn create(&self, comment: &TaskComment) -> AppResult<TaskComment> {
        let query = format!(
            "INSERT INTO comments (task_id, author_user_id, kind, body, created_at)
             VALUES (?, ?, ?, ?, ?)
             RETURNING {COMMENT_COLUMNS}"
        );
        let row = sqlx::query_as::<_, CommentRow>(&query)
            .bind(comment.task_id)
            .bind(comment.author_user_id)
            .bind(comment_kind_to_db(comment.kind))
            .bind(&comment.body)
            .bind(comment.created_at)
            .fetch_one(&self.pool)
            .await
            .map_err(database_error)?;
        row.try_into()
    }

    async fn list_recent_for_task(&self, task_id: i64, limit: i64) -> AppResult<Vec<TaskComment>> {
        let query = format!(
            "SELECT {COMMENT_COLUMNS} FROM comments
             WHERE task_id = ?
             ORDER BY created_at DESC, id DESC
             LIMIT ?"
        );
        let rows = sqlx::query_as::<_, CommentRow>(&query)
            .bind(task_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await
            .map_err(database_error)?;
        rows.into_iter().map(TryInto::try_into).collect()
    }
}
