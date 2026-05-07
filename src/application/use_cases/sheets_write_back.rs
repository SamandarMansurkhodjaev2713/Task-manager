//! Background use case: flush the `pending_sheet_writes` queue to Google
//! Sheets.
//!
//! # Lifecycle
//!
//! 1. During onboarding, `SheetsWriteBackUseCase::enqueue` inserts a row into
//!    `pending_sheet_writes` for every `bot_registered` employee who completes
//!    `/start`.
//! 2. The background scheduler calls `SheetsWriteBackUseCase::flush` on a
//!    periodic interval.  Each invocation processes up to
//!    `MAX_FLUSH_BATCH_SIZE` pending rows.
//! 3. For each row the gateway is called exactly once.  On success,
//!    `written_at` is stamped.  On failure, `error_count` is incremented and
//!    `next_attempt_at` is set to `now + 2^error_count minutes` (capped at 4 h)
//!    so the worker observes exponential back-off instead of hammering the
//!    Sheets API on every tick.
//! 4. Rows that reach `MAX_WRITE_BACK_ATTEMPTS` are permanently skipped; the
//!    `sheets_write_back_abandoned_total` counter is incremented so operators
//!    can alert on runaway failures.
//!
//! # No-op mode
//!
//! When no `SheetsWriteBackGateway` is injected (i.e. `write_back_gateway` is
//! `None`), both `enqueue` and `flush` are pure no-ops.  This means the whole
//! feature is safely disabled when the operator has not configured
//! `GOOGLE_SHEETS_WRITE_BACK_RANGE`.

use std::sync::Arc;

use chrono::Utc;
use metrics::counter;

use crate::application::ports::repositories::SheetsSyncRepository;
use crate::application::ports::services::SheetsWriteBackGateway;
use crate::domain::errors::AppResult;

/// Maximum number of consecutive write failures before a row is abandoned.
pub const MAX_WRITE_BACK_ATTEMPTS: u32 = 5;
/// Number of rows processed per `flush` call.
const MAX_FLUSH_BATCH_SIZE: i64 = 20;

pub struct SheetsWriteBackUseCase {
    sync_repository: Arc<dyn SheetsSyncRepository>,
    /// `None` when write-back is not configured (no gateway / range missing).
    gateway: Option<Arc<dyn SheetsWriteBackGateway>>,
}

impl SheetsWriteBackUseCase {
    /// Create a use case that actively writes back to Sheets.
    pub fn new(
        sync_repository: Arc<dyn SheetsSyncRepository>,
        gateway: Arc<dyn SheetsWriteBackGateway>,
    ) -> Self {
        Self {
            sync_repository,
            gateway: Some(gateway),
        }
    }

    /// Create a no-op use case (write-back not configured).
    pub fn disabled(sync_repository: Arc<dyn SheetsSyncRepository>) -> Self {
        Self {
            sync_repository,
            gateway: None,
        }
    }

    /// Enqueue a `bot_registered` employee for write-back.
    ///
    /// This is called from the presentation layer immediately after onboarding
    /// completes.  It is idempotent: calling twice for the same `employee_id`
    /// silently keeps only one pending row.
    ///
    /// When write-back is disabled this is a fast no-op.
    pub async fn enqueue(
        &self,
        employee_id: i64,
        telegram_id: i64,
        full_name: &str,
        telegram_username: Option<&str>,
    ) -> AppResult<()> {
        if self.gateway.is_none() {
            return Ok(());
        }
        self.sync_repository
            .enqueue(employee_id, telegram_id, full_name, telegram_username)
            .await
    }

    /// Flush up to `MAX_FLUSH_BATCH_SIZE` pending rows to Google Sheets.
    ///
    /// Called by the background scheduler.  Rows whose exponential back-off
    /// window has not yet expired (`next_attempt_at > now`) are silently
    /// skipped by the repository query — no application-layer filtering needed.
    /// Rows that reach `MAX_WRITE_BACK_ATTEMPTS` are never returned by
    /// `list_pending` and are effectively abandoned; the
    /// `sheets_write_back_abandoned_total` counter fires on the last failure
    /// so operators can alert.
    pub async fn flush(&self) -> AppResult<()> {
        let Some(gateway) = &self.gateway else {
            return Ok(());
        };

        let now = Utc::now();
        let pending = self
            .sync_repository
            .list_pending(MAX_WRITE_BACK_ATTEMPTS, MAX_FLUSH_BATCH_SIZE, now)
            .await?;

        for row in pending {
            match gateway
                .append_employee_row(
                    &row.full_name,
                    row.telegram_username.as_deref(),
                    row.telegram_id,
                )
                .await
            {
                Ok(()) => {
                    if let Err(error) = self.sync_repository.mark_written(row.id, now).await {
                        tracing::error!(
                            row_id = row.id,
                            code = error.code(),
                            "sheets_write_back: failed to mark row as written"
                        );
                    }
                    tracing::info!(
                        employee_id = row.employee_id,
                        full_name = %row.full_name,
                        "sheets_write_back: row written successfully"
                    );
                }
                Err(error) => {
                    let new_error_count = row.error_count + 1;
                    tracing::warn!(
                        employee_id = row.employee_id,
                        attempt = new_error_count,
                        max = MAX_WRITE_BACK_ATTEMPTS,
                        code = error.code(),
                        "sheets_write_back: append failed"
                    );
                    if let Err(inner) = self
                        .sync_repository
                        .record_error(row.id, error.code(), now)
                        .await
                    {
                        tracing::error!(
                            row_id = row.id,
                            code = inner.code(),
                            "sheets_write_back: failed to record error"
                        );
                    }
                    // Emit an observability signal when a row has exhausted all
                    // attempts.  The next flush will silently skip it via the
                    // `error_count < max_attempts` filter.
                    if new_error_count >= MAX_WRITE_BACK_ATTEMPTS {
                        tracing::error!(
                            employee_id = row.employee_id,
                            full_name = %row.full_name,
                            attempts = new_error_count,
                            "sheets_write_back: row permanently abandoned after max attempts"
                        );
                        counter!("sheets_write_back_abandoned_total").increment(1);
                    }
                }
            }
        }

        Ok(())
    }
}
