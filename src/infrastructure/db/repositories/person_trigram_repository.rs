//! SQLite adapter for [`PersonTrigramIndex`] — Phase 5 skeleton.
//!
//! Scope of this Phase 5 cut:
//! * Storage: full CRUD over `person_trigrams` (migration 009).
//! * Lookup: unweighted trigram-overlap ranking, good enough for a
//!   30-person directory and intended as a drop-in baseline before
//!   tuning score weights in a later phase.
//! * The adapter writes through a single transaction per `upsert` so the
//!   trigram set for an owner never goes into a half-rebuilt state that
//!   the lookup can observe.
//!
//! What's intentionally **not** here yet (future phases):
//! * No rebuild-on-signal job: higher-level use cases will call `upsert`
//!   directly when employee/user names change.
//! * No TF-IDF / BM25 weighting.  That is measured against a benchmark
//!   dataset in Phase 10, and would be premature without one.

use serde_json::json;
use sqlx::SqlitePool;

use crate::application::ports::repositories::{
    PersonTrigramCandidate, PersonTrigramIndex, PersonTrigramOwnerKind,
};
use crate::domain::errors::{AppError, AppResult};

use super::common::database_error;

/// Hard cap on the number of results we rank in one call.  Keeps SQLite
/// from ever scanning an unbounded number of candidate rows, which the
/// caller could otherwise trigger with a large `limit`.
const MAX_TOP_MATCHES: u32 = 50;
const MIN_TOP_MATCHES: u32 = 1;

#[derive(Clone)]
pub struct SqlitePersonTrigramIndex {
    pool: SqlitePool,
}

impl SqlitePersonTrigramIndex {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl PersonTrigramIndex for SqlitePersonTrigramIndex {
    async fn upsert(
        &self,
        owner_kind: PersonTrigramOwnerKind,
        owner_id: i64,
        trigrams: &[String],
    ) -> AppResult<()> {
        let mut tx = self.pool.begin().await.map_err(database_error)?;

        sqlx::query("DELETE FROM person_trigrams WHERE owner_kind = ? AND owner_id = ?")
            .bind(owner_kind.as_code())
            .bind(owner_id)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;

        // Deduplicate at the application layer so we don't depend on the
        // database emitting a conflict on the unique index — the unique
        // index is a safety net, not a perf-critical path.
        let mut seen = std::collections::HashSet::new();
        for trigram in trigrams {
            if !seen.insert(trigram.clone()) {
                continue;
            }
            sqlx::query(
                "INSERT INTO person_trigrams (owner_kind, owner_id, trigram)
                 VALUES (?, ?, ?)",
            )
            .bind(owner_kind.as_code())
            .bind(owner_id)
            .bind(trigram)
            .execute(&mut *tx)
            .await
            .map_err(database_error)?;
        }

        tx.commit().await.map_err(database_error)?;
        Ok(())
    }

    async fn delete(&self, owner_kind: PersonTrigramOwnerKind, owner_id: i64) -> AppResult<()> {
        sqlx::query("DELETE FROM person_trigrams WHERE owner_kind = ? AND owner_id = ?")
            .bind(owner_kind.as_code())
            .bind(owner_id)
            .execute(&self.pool)
            .await
            .map_err(database_error)?;
        Ok(())
    }

    async fn top_matches(
        &self,
        query_trigrams: &[String],
        limit: u32,
    ) -> AppResult<Vec<PersonTrigramCandidate>> {
        if query_trigrams.is_empty() {
            return Ok(Vec::new());
        }

        let effective_limit = limit.clamp(MIN_TOP_MATCHES, MAX_TOP_MATCHES) as i64;

        // Build the `(?, ?, ...)` placeholder list at call time; SQLite's
        // driver does not natively support `IN (?)` over a `&[String]`.
        let placeholders = std::iter::repeat_n("?", query_trigrams.len())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "SELECT owner_kind, owner_id, COUNT(*) AS shared
             FROM person_trigrams
             WHERE trigram IN ({placeholders})
             GROUP BY owner_kind, owner_id
             ORDER BY shared DESC
             LIMIT ?"
        );

        let mut query = sqlx::query_as::<_, (String, i64, i64)>(&sql);
        for trigram in query_trigrams {
            query = query.bind(trigram);
        }
        query = query.bind(effective_limit);

        let rows = query.fetch_all(&self.pool).await.map_err(database_error)?;

        // We compute the score in Rust (not SQL) because the formula is
        // cheap and the caller-visible representation is what other
        // subsystems compose with.
        let query_size = query_trigrams.len() as u32;
        let mut out = Vec::with_capacity(rows.len());
        for (owner_kind_code, owner_id, shared) in rows {
            let owner_kind = parse_owner_kind(&owner_kind_code)?;
            let shared_u32 = shared.max(0) as u32;
            let score = if query_size == 0 {
                0.0
            } else {
                shared_u32 as f32 / query_size as f32
            };
            out.push(PersonTrigramCandidate {
                owner_kind,
                owner_id,
                shared_trigrams: shared_u32,
                score,
            });
        }

        Ok(out)
    }
}

fn parse_owner_kind(code: &str) -> AppResult<PersonTrigramOwnerKind> {
    match code {
        "employee" => Ok(PersonTrigramOwnerKind::Employee),
        "user" => Ok(PersonTrigramOwnerKind::User),
        other => Err(AppError::internal(
            "PERSON_TRIGRAM_OWNER_KIND_UNKNOWN",
            format!("unrecognised person_trigrams.owner_kind value '{other}' — migration drift?"),
            json!({ "owner_kind": other }),
        )),
    }
}
