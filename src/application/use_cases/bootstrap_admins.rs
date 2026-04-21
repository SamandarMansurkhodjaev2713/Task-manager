//! `/start`-independent admin bootstrapping.
//!
//! On every process start we take the contents of `TELEGRAM_ADMIN_IDS`
//! (materialised as [`AdminIdSet`](crate::config::AdminIdSet)) and ensure
//! that every Telegram ID it contains has role=Admin in the `users`
//! table — **provided that the user already exists**.  A user that has
//! not yet sent `/start` is simply skipped; the next `/start` they send
//! will either promote them via onboarding (if still listed) or leave
//! them as a regular user (if they were removed from `.env`).
//!
//! Invariants:
//!   * Idempotent: running the use case twice with the same set must
//!     produce no new side effects beyond logging the no-op.
//!   * Auditable: every successful **elevation** writes an
//!     [`AdminAuditEntry`] with code [`AuditActionCode::RoleElevatedByBootstrap`].
//!   * Resilient: failures to promote one user MUST NOT abort the
//!     remaining promotions — we log the error and continue.
//!
//! This use case is intentionally *not* wired to any HTTP/Telegram
//! endpoint — it is called once during application bootstrap from
//! [`crate::presentation::bootstrap::run_application`].

use std::sync::Arc;

use serde_json::json;
use tracing::{info, warn};

use crate::application::ports::repositories::{
    AdminAuditLogRepository, BootstrapPromotion, UserRepository,
};
use crate::application::ports::services::Clock;
use crate::config::AdminIdSet;
use crate::domain::audit::{AdminAuditEntry, AuditActionCode};
use crate::domain::errors::AppResult;

/// Summary of a single bootstrap sweep.  Returned from
/// [`BootstrapAdminsUseCase::execute`] for logging and metrics.
#[derive(Debug, Default, Clone, Copy)]
pub struct BootstrapSummary {
    pub configured_admins: usize,
    pub elevated_now: usize,
    pub already_admin: usize,
    pub pending_signup: usize,
    pub failed: usize,
}

pub struct BootstrapAdminsUseCase {
    clock: Arc<dyn Clock>,
    user_repository: Arc<dyn UserRepository>,
    admin_audit_repository: Arc<dyn AdminAuditLogRepository>,
}

impl BootstrapAdminsUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        user_repository: Arc<dyn UserRepository>,
        admin_audit_repository: Arc<dyn AdminAuditLogRepository>,
    ) -> Self {
        Self {
            clock,
            user_repository,
            admin_audit_repository,
        }
    }

    /// Promotes every Telegram ID in `admin_ids` to role=Admin.  Users
    /// that have never sent `/start` are skipped; elevation failures are
    /// logged but never abort the sweep.
    pub async fn execute(&self, admin_ids: &AdminIdSet) -> AppResult<BootstrapSummary> {
        let mut summary = BootstrapSummary {
            configured_admins: admin_ids.len(),
            ..BootstrapSummary::default()
        };

        if admin_ids.is_empty() {
            warn!(
                target = "rbac.bootstrap",
                "TELEGRAM_ADMIN_IDS is empty; no admin will be provisioned automatically"
            );
            return Ok(summary);
        }

        let now = self.clock.now_utc();
        for telegram_id in admin_ids.iter() {
            match self
                .user_repository
                .promote_bootstrap_admin(telegram_id, now)
                .await
            {
                Ok(None) => {
                    summary.pending_signup += 1;
                    info!(
                        target = "rbac.bootstrap",
                        telegram_id, "admin pending first /start; will be promoted after signup"
                    );
                }
                Ok(Some(BootstrapPromotion::AlreadyAdmin(user))) => {
                    summary.already_admin += 1;
                    info!(
                        target = "rbac.bootstrap",
                        telegram_id,
                        user_id = user.id.unwrap_or_default(),
                        "admin already provisioned"
                    );
                }
                Ok(Some(BootstrapPromotion::Elevated(user))) => {
                    summary.elevated_now += 1;
                    info!(
                        target = "rbac.bootstrap",
                        telegram_id,
                        user_id = user.id.unwrap_or_default(),
                        "admin elevated by bootstrap"
                    );
                    // Audit failures must not abort bootstrap — the role
                    // change is already persisted.  We log and move on.
                    let entry = AdminAuditEntry {
                        id: None,
                        actor_user_id: None,
                        target_user_id: user.id,
                        action_code: AuditActionCode::RoleElevatedByBootstrap,
                        metadata: json!({
                            "telegram_id": telegram_id,
                            "source": "env:TELEGRAM_ADMIN_IDS",
                        }),
                        created_at: now,
                    };
                    if let Err(error) = self.admin_audit_repository.append(&entry).await {
                        warn!(
                            target = "rbac.bootstrap",
                            telegram_id,
                            error = %error,
                            "failed to persist admin audit entry for bootstrap elevation"
                        );
                    }
                }
                Err(error) => {
                    summary.failed += 1;
                    warn!(
                        target = "rbac.bootstrap",
                        telegram_id,
                        error = %error,
                        "failed to promote bootstrap admin; continuing"
                    );
                }
            }
        }

        info!(
            target = "rbac.bootstrap",
            configured = summary.configured_admins,
            elevated = summary.elevated_now,
            already_admin = summary.already_admin,
            pending_signup = summary.pending_signup,
            failed = summary.failed,
            "admin bootstrap sweep completed"
        );
        Ok(summary)
    }
}
