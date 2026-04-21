use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::Mutex as AsyncMutex;

use crate::application::dto::task_views::StatsView;
use crate::application::policies::role_authorization::RoleAuthorizationPolicy;
use crate::application::ports::repositories::TaskRepository;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::task::TaskStats;
use crate::domain::user::User;

/// Global stats are expensive (full-table COUNT) and change infrequently —
/// cache the result for this duration before hitting the DB again.
const GLOBAL_STATS_TTL: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsScope {
    Personal,
    Team,
}

pub struct CollectStatsUseCase {
    task_repository: Arc<dyn TaskRepository>,
    /// Short-lived in-memory cache for the expensive global stats query.
    /// `None` means the cache is cold; `Some((fetched_at, stats))` means it
    /// is warm and valid until `fetched_at + GLOBAL_STATS_TTL`.
    global_cache: Arc<AsyncMutex<Option<(Instant, TaskStats)>>>,
}

impl CollectStatsUseCase {
    pub fn new(task_repository: Arc<dyn TaskRepository>) -> Self {
        Self {
            task_repository,
            global_cache: Arc::new(AsyncMutex::new(None)),
        }
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
                RoleAuthorizationPolicy::ensure_can_view_team_stats(actor)?;
                self.global_stats_cached().await?
            }
        };
        Ok(stats.into())
    }

    /// Returns global stats from the cache if the cached value is still fresh;
    /// otherwise re-queries the DB, updates the cache, and returns the result.
    async fn global_stats_cached(&self) -> AppResult<TaskStats> {
        let mut guard = self.global_cache.lock().await;

        if let Some((fetched_at, ref stats)) = *guard {
            if fetched_at.elapsed() < GLOBAL_STATS_TTL {
                return Ok(stats.clone());
            }
        }

        let fresh = self.task_repository.count_stats_global().await?;
        *guard = Some((Instant::now(), fresh.clone()));
        Ok(fresh)
    }
}
