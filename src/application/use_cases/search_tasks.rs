//! Task search use case (Phase 10 skeleton).
//!
//! The full search stack (trigram ranking, assignee scoping, keyset
//! pagination, filter DSL) is tracked for a future phase.  This module
//! intentionally delivers a minimal, production-safe vertical slice:
//!
//! * Input is validated and normalised via [`SearchQuery::parse`].
//! * The use case does an indexed substring match against the caller's own
//!   tasks (so we can never leak other users' data).
//! * Results are clamped to `MAX_SEARCH_RESULTS` and converted to the same
//!   DTO shape the existing list screens use.
//!
//! The skeleton allows the `/find <query>` command to work today without
//! committing us to the eventual API: we can swap out the filter clause
//! for an FTS5 or trigram-index query later without touching the
//! presentation layer.

use std::sync::Arc;

use tracing::instrument;

use crate::application::dto::task_views::TaskListItem;
use crate::application::ports::repositories::TaskRepository;
use crate::domain::errors::{AppError, AppResult};
use crate::domain::task::Task;
use crate::domain::user::User;

/// Hard cap on results returned by the skeleton search.  Keeps Telegram
/// payloads small (fewer than 3 pages of text) and the SQL scan bounded.
pub const MAX_SEARCH_RESULTS: usize = 15;
pub const MIN_QUERY_CHARS: usize = 2;
pub const MAX_QUERY_CHARS: usize = 64;

/// Validated search query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchQuery {
    canonical: String,
}

impl SearchQuery {
    pub fn parse(raw: &str) -> AppResult<Self> {
        let canonical = raw
            .chars()
            .filter(|character| !character.is_control())
            .collect::<String>()
            .trim()
            .to_lowercase();
        if canonical.is_empty() {
            return Err(AppError::schema_validation(
                "SEARCH_QUERY_EMPTY",
                "search query must not be empty",
                serde_json::json!({}),
            ));
        }
        let char_count = canonical.chars().count();
        if char_count < MIN_QUERY_CHARS {
            return Err(AppError::schema_validation(
                "SEARCH_QUERY_TOO_SHORT",
                format!("query must be at least {MIN_QUERY_CHARS} characters"),
                serde_json::json!({ "min": MIN_QUERY_CHARS }),
            ));
        }
        if char_count > MAX_QUERY_CHARS {
            return Err(AppError::schema_validation(
                "SEARCH_QUERY_TOO_LONG",
                format!("query must be at most {MAX_QUERY_CHARS} characters"),
                serde_json::json!({ "max": MAX_QUERY_CHARS }),
            ));
        }
        Ok(Self { canonical })
    }

    pub fn canonical(&self) -> &str {
        &self.canonical
    }
}

/// Search-use-case skeleton.  Delegates persistence to `TaskRepository`
/// and filters the caller-visible subset in memory — acceptable for a
/// 30-employee fleet and easy to replace with a server-side predicate.
pub struct SearchTasksUseCase {
    task_repo: Arc<dyn TaskRepository>,
}

impl SearchTasksUseCase {
    pub fn new(task_repo: Arc<dyn TaskRepository>) -> Self {
        Self { task_repo }
    }

    /// Executes a substring search against the tasks visible to `actor`.
    ///
    /// Searches both tasks assigned to the actor **and** tasks created by the
    /// actor so that a manager who creates tasks on behalf of others can still
    /// find their own work.  Results are deduplicated by `task_uid` (a task
    /// that is both created by and assigned to the same user appears only once).
    #[instrument(skip_all, fields(user_id = actor.id, query_len = query.canonical().chars().count()))]
    pub async fn execute(&self, actor: &User, query: &SearchQuery) -> AppResult<Vec<TaskListItem>> {
        let Some(user_id) = actor.id else {
            return Err(AppError::forbidden(
                "SEARCH_REQUIRES_PERSISTED_USER",
                "каталог задач доступен только зарегистрированным пользователям",
                serde_json::json!({}),
            ));
        };

        // Fetch both queues and merge client-side.  For ≤40 employees this is
        // well under a second even with a few thousand tasks total.
        let assigned = self
            .task_repo
            .list_assigned_to_user(user_id, None, 200)
            .await?;
        let created = self
            .task_repo
            .list_created_by_user(user_id, None, 200)
            .await?;

        // Deduplicate by task_uid: a task that is both created by AND assigned
        // to the same user must appear only once in results.
        let mut seen = std::collections::HashSet::new();
        let merged: Vec<Task> = assigned
            .into_iter()
            .chain(created)
            .filter(|task| seen.insert(task.task_uid))
            .collect();

        let canonical = query.canonical();
        let mut matching: Vec<Task> = merged
            .into_iter()
            .filter(|task| matches_query(task, canonical))
            .collect();

        // Stable ordering by updated_at desc so the UI feels predictable.
        matching.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        matching.truncate(MAX_SEARCH_RESULTS);

        let results = matching
            .iter()
            .map(|task| TaskListItem::from_task(task, None, None, None))
            .collect();
        Ok(results)
    }
}

fn matches_query(task: &Task, canonical_query: &str) -> bool {
    let haystack_title = task.title.to_lowercase();
    if haystack_title.contains(canonical_query) {
        return true;
    }
    let haystack_desc = task.description.to_lowercase();
    if haystack_desc.contains(canonical_query) {
        return true;
    }
    if task
        .task_uid
        .to_string()
        .to_lowercase()
        .contains(canonical_query)
    {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{SearchQuery, MAX_QUERY_CHARS, MIN_QUERY_CHARS};

    #[test]
    fn given_valid_query_when_parsed_then_normalised_to_lowercase() {
        let q = SearchQuery::parse("  RELease  ").expect("valid");
        assert_eq!(q.canonical(), "release");
    }

    const _INVARIANT_MIN_QUERY_CHARS: () = assert!(MIN_QUERY_CHARS >= 2);

    #[test]
    fn given_short_query_when_parsed_then_rejected() {
        let err = SearchQuery::parse("а").unwrap_err();
        assert_eq!(err.code(), "SEARCH_QUERY_TOO_SHORT");
    }

    #[test]
    fn given_empty_query_when_parsed_then_rejected() {
        let err = SearchQuery::parse("   ").unwrap_err();
        assert_eq!(err.code(), "SEARCH_QUERY_EMPTY");
    }

    #[test]
    fn given_too_long_query_when_parsed_then_rejected() {
        let long = "я".repeat(MAX_QUERY_CHARS + 1);
        let err = SearchQuery::parse(&long).unwrap_err();
        assert_eq!(err.code(), "SEARCH_QUERY_TOO_LONG");
    }
}
