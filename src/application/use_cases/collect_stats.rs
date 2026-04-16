use std::sync::Arc;

use crate::application::dto::task_views::StatsView;
use crate::application::ports::repositories::TaskRepository;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::user::User;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsScope {
    Personal,
    Team,
}

pub struct CollectStatsUseCase {
    task_repository: Arc<dyn TaskRepository>,
}

impl CollectStatsUseCase {
    pub fn new(task_repository: Arc<dyn TaskRepository>) -> Self {
        Self { task_repository }
    }

    pub async fn execute(&self, actor: &User, scope: StatsScope) -> AppResult<StatsView> {
        let actor_id = actor.id.ok_or_else(|| {
            AppError::unauthenticated(
                "User must be registered before viewing stats",
                serde_json::json!({ "telegram_id": actor.telegram_id }),
            )
        })?;

        let stats = match scope {
            StatsScope::Personal => self.task_repository.count_stats_for_user(actor_id).await?,
            StatsScope::Team => {
                if !actor.role.is_manager_or_admin() {
                    return Err(AppError::unauthorized(
                        "Only managers and admins can view team stats",
                        serde_json::json!({ "telegram_id": actor.telegram_id }),
                    ));
                }
                self.task_repository.count_stats_global().await?
            }
        };
        Ok(stats.into())
    }
}
