//! Integration tests for `SqlitePersonTrigramIndex` (Phase 5 skeleton).
//!
//! These tests run against a fresh SQLite database created from `migrations/`
//! so they also act as smoke tests for migration 009.

use tempfile::{tempdir, TempDir};

use telegram_task_bot::application::ports::repositories::{
    PersonTrigramIndex, PersonTrigramOwnerKind,
};
use telegram_task_bot::domain::person_name::PersonName;
use telegram_task_bot::infrastructure::db::pool::connect;
use telegram_task_bot::infrastructure::db::repositories::SqlitePersonTrigramIndex;

async fn test_pool() -> (TempDir, sqlx::SqlitePool) {
    let temp_dir = tempdir().expect("temp dir should be created");
    let db_path = temp_dir.path().join("trigrams.db");
    let database_url = format!("sqlite://{}", db_path.to_string_lossy().replace('\\', "/"));
    let pool = connect(&database_url)
        .await
        .expect("database should connect");
    (temp_dir, pool)
}

#[tokio::test]
async fn given_two_owners_when_query_matches_one_then_scores_it_highest() {
    let (_temp, pool) = test_pool().await;
    let index = SqlitePersonTrigramIndex::new(pool);

    let ivan = PersonName::parse("Иван", "Иванов").expect("valid name");
    let petr = PersonName::parse("Петр", "Петров").expect("valid name");

    index
        .upsert(PersonTrigramOwnerKind::User, 1, &ivan.trigrams())
        .await
        .expect("upsert should succeed");
    index
        .upsert(PersonTrigramOwnerKind::User, 2, &petr.trigrams())
        .await
        .expect("upsert should succeed");

    let query = ivan.trigrams();
    let matches = index
        .top_matches(&query, 5)
        .await
        .expect("top_matches should succeed");

    assert!(!matches.is_empty(), "at least one owner must match");
    assert_eq!(matches[0].owner_id, 1);
    assert!(matches[0].score > 0.0);
    assert!(
        matches.iter().all(|candidate| candidate.score <= 1.0),
        "scores must lie in [0, 1]"
    );
}

#[tokio::test]
async fn given_owner_when_upsert_twice_then_second_replaces_first() {
    let (_temp, pool) = test_pool().await;
    let index = SqlitePersonTrigramIndex::new(pool.clone());

    let first = PersonName::parse("Иван", "Иванов").expect("valid name");
    let second = PersonName::parse("Петр", "Петров").expect("valid name");

    index
        .upsert(PersonTrigramOwnerKind::User, 42, &first.trigrams())
        .await
        .expect("first upsert should succeed");
    index
        .upsert(PersonTrigramOwnerKind::User, 42, &second.trigrams())
        .await
        .expect("second upsert should succeed");

    let stored: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM person_trigrams WHERE owner_id = ?")
        .bind(42_i64)
        .fetch_one(&pool)
        .await
        .expect("count should succeed");

    let expected_distinct = second
        .trigrams()
        .into_iter()
        .collect::<std::collections::HashSet<_>>()
        .len();
    assert_eq!(
        stored as usize, expected_distinct,
        "upsert must replace the old trigram set with the deduplicated new one"
    );
}

#[tokio::test]
async fn given_empty_query_when_top_matches_then_returns_empty() {
    let (_temp, pool) = test_pool().await;
    let index = SqlitePersonTrigramIndex::new(pool);

    let matches = index
        .top_matches(&[], 5)
        .await
        .expect("top_matches with empty query should succeed");

    assert!(matches.is_empty());
}

#[tokio::test]
async fn given_owner_when_delete_then_rows_gone() {
    let (_temp, pool) = test_pool().await;
    let index = SqlitePersonTrigramIndex::new(pool.clone());

    let ivan = PersonName::parse("Иван", "Иванов").expect("valid name");
    index
        .upsert(PersonTrigramOwnerKind::Employee, 7, &ivan.trigrams())
        .await
        .expect("upsert should succeed");

    index
        .delete(PersonTrigramOwnerKind::Employee, 7)
        .await
        .expect("delete should succeed");

    let remaining: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM person_trigrams WHERE owner_id = ?")
            .bind(7_i64)
            .fetch_one(&pool)
            .await
            .expect("count should succeed");

    assert_eq!(remaining, 0);
}
