//! In-Telegram admin-panel use cases.
//!
//! All admin panel commands flow through this module so the policy checks,
//! audit writes, and the last-admin invariant are enforced in exactly one
//! place.  The presentation layer (`dispatcher_admin.rs`) is thin: it
//! translates Telegram callbacks into these use cases, then renders the
//! resulting `User` / `Vec<User>` back to the chat.
//!
//! Invariants this module must uphold:
//!
//! * Every admin action is authorised via `RoleAuthorizationPolicy` and the
//!   "active" check — a deactivated admin cannot mutate roles.
//! * Every successful mutation produces an [`AdminAuditEntry`].
//!   Audit writes that fail are logged but do NOT rollback the business
//!   action; the underlying row is the source of truth and the audit is an
//!   observability artifact.
//! * All text payloads the admin sees (display names) are truncated so a
//!   long display_name cannot cause a Telegram "Message too long" failure
//!   via the audit metadata.

use std::sync::Arc;

use serde_json::json;
use tracing::warn;

use crate::application::policies::role_authorization::RoleAuthorizationPolicy;
use crate::application::ports::repositories::{
    AdminAuditLogRepository, FeatureFlagRepository, UserRepository,
};
use crate::application::ports::services::Clock;
use crate::domain::audit::{AdminAuditEntry, AuditActionCode};
use crate::domain::errors::{AppError, AppResult};
use crate::domain::user::{User, UserRole};
use crate::shared::feature_flags::{FeatureFlag, SharedFeatureFlagRegistry, UnknownFeatureFlag};

pub const USER_NOT_FOUND: &str = "USER_NOT_FOUND";
pub const ACTOR_NOT_FOUND: &str = "ACTOR_NOT_FOUND";

pub struct AdminUseCase {
    clock: Arc<dyn Clock>,
    user_repository: Arc<dyn UserRepository>,
    admin_audit_repository: Arc<dyn AdminAuditLogRepository>,
    feature_flag_repository: Arc<dyn FeatureFlagRepository>,
    /// Shared in-memory registry mutated on successful toggle so that the new
    /// flag state is visible to the rest of the process without a restart.
    shared_flags: SharedFeatureFlagRegistry,
}

impl AdminUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        user_repository: Arc<dyn UserRepository>,
        admin_audit_repository: Arc<dyn AdminAuditLogRepository>,
        feature_flag_repository: Arc<dyn FeatureFlagRepository>,
        shared_flags: SharedFeatureFlagRegistry,
    ) -> Self {
        Self {
            clock,
            user_repository,
            admin_audit_repository,
            feature_flag_repository,
            shared_flags,
        }
    }

    /// Lists the currently active admins.  The caller is expected to
    /// already be an admin; we still re-check here for defence in depth.
    pub async fn list_active_admins(&self, actor: &User) -> AppResult<Vec<User>> {
        RoleAuthorizationPolicy::ensure_can_view_admin_panel(actor)?;
        self.user_repository.list_active_admins().await
    }

    /// Fetches a single user by primary key, re-authorising the caller.
    /// Returns a dedicated `USER_NOT_FOUND` error so the UI can show a
    /// polite message rather than a generic "internal error".
    pub async fn get_user(&self, actor: &User, target_user_id: i64) -> AppResult<User> {
        RoleAuthorizationPolicy::ensure_can_view_admin_panel(actor)?;
        self.user_repository
            .find_by_id(target_user_id)
            .await?
            .ok_or_else(|| {
                AppError::not_found(
                    USER_NOT_FOUND,
                    "Requested user does not exist",
                    json!({ "target_user_id": target_user_id }),
                )
            })
    }

    /// Changes a user's role, writing an audit entry on success.  Delegates
    /// the last-admin invariant check to `UserRepository::set_role` —
    /// which returns `LAST_ADMIN_PROTECTED` when the action would leave
    /// the system without any active admin.
    pub async fn change_role(
        &self,
        actor: &User,
        target_user_id: i64,
        next_role: UserRole,
    ) -> AppResult<User> {
        let target = self.get_user(actor, target_user_id).await?;
        RoleAuthorizationPolicy::ensure_can_manage_roles(actor, &target)?;

        let actor_id = actor
            .id
            .ok_or_else(|| AppError::unauthenticated("Actor must be registered", json!({})))?;

        let now = self.clock.now_utc();
        let previous_role = target.role;
        let updated = self
            .user_repository
            .set_role(actor_id, target_user_id, next_role, now)
            .await?;

        let entry = AdminAuditEntry {
            id: None,
            actor_user_id: actor.id,
            target_user_id: Some(target_user_id),
            action_code: AuditActionCode::RoleChangedByAdmin,
            metadata: json!({
                "previous_role": previous_role.to_string(),
                "next_role": next_role.to_string(),
                "target_telegram_id": target.telegram_id,
            }),
            created_at: now,
        };
        self.emit_audit(entry).await;
        Ok(updated)
    }

    /// Deactivates a user account (soft delete).  Last-admin invariant is
    /// re-enforced by the repository.
    pub async fn deactivate_user(&self, actor: &User, target_user_id: i64) -> AppResult<User> {
        let target = self.get_user(actor, target_user_id).await?;
        RoleAuthorizationPolicy::ensure_can_deactivate_user(actor, &target)?;

        let actor_id = actor
            .id
            .ok_or_else(|| AppError::unauthenticated("Actor must be registered", json!({})))?;
        let now = self.clock.now_utc();
        let updated = self
            .user_repository
            .deactivate(actor_id, target_user_id, now)
            .await?;

        self.emit_audit(AdminAuditEntry {
            id: None,
            actor_user_id: actor.id,
            target_user_id: Some(target_user_id),
            action_code: AuditActionCode::UserDeactivatedByAdmin,
            metadata: json!({
                "target_telegram_id": target.telegram_id,
                "previous_role": target.role.to_string(),
            }),
            created_at: now,
        })
        .await;
        Ok(updated)
    }

    /// Reactivates a user.  Idempotent — calling on an active user is a
    /// silent no-op from the DB's perspective, but still emits an audit
    /// entry so ops can see the request happened.
    pub async fn reactivate_user(&self, actor: &User, target_user_id: i64) -> AppResult<User> {
        let target = self.get_user(actor, target_user_id).await?;
        RoleAuthorizationPolicy::ensure_can_access_admin_panel(actor)?;

        let actor_id = actor
            .id
            .ok_or_else(|| AppError::unauthenticated("Actor must be registered", json!({})))?;
        let now = self.clock.now_utc();
        let updated = self
            .user_repository
            .reactivate(actor_id, target_user_id, now)
            .await?;

        self.emit_audit(AdminAuditEntry {
            id: None,
            actor_user_id: actor.id,
            target_user_id: Some(target_user_id),
            action_code: AuditActionCode::UserReactivatedByAdmin,
            metadata: json!({
                "target_telegram_id": target.telegram_id,
            }),
            created_at: now,
        })
        .await;
        Ok(updated)
    }

    /// Fetches the last N admin audit entries.  Authorised via the
    /// read-only admin gate.  `limit` is clamped to `[1, 50]` — the admin
    /// panel shows a single scrollable screen so larger requests would be
    /// ignored by the UI anyway.
    pub async fn list_recent_audit(
        &self,
        actor: &User,
        limit: u32,
    ) -> AppResult<Vec<AdminAuditEntry>> {
        RoleAuthorizationPolicy::ensure_can_view_admin_panel(actor)?;
        let clamped = limit.clamp(1, 50) as i64;
        self.admin_audit_repository.list_recent(clamped).await
    }

    /// Toggles a feature flag by key.
    ///
    /// * Parses and validates `flag_key` (returns `UNKNOWN_FEATURE_FLAG` if
    ///   the key is not in the known set — guards against stale callbacks).
    /// * Persists the new state to `feature_flag_overrides`.
    /// * Atomically updates the in-memory [`SharedFeatureFlagRegistry`] so the
    ///   change takes effect immediately without a process restart.
    /// * Emits an `AdminFeatureToggled` audit entry.
    ///
    /// Returns the new enabled state.
    pub async fn toggle_feature_flag(
        &self,
        actor: &User,
        flag_key: &str,
        enabled: bool,
    ) -> AppResult<bool> {
        RoleAuthorizationPolicy::ensure_can_access_admin_panel(actor)?;

        let flag = flag_key
            .parse::<FeatureFlag>()
            .map_err(|UnknownFeatureFlag(key)| {
                AppError::not_found(
                    "UNKNOWN_FEATURE_FLAG",
                    "feature flag key is not recognised",
                    json!({ "flag_key": key }),
                )
            })?;

        let actor_id = actor
            .id
            .ok_or_else(|| AppError::unauthenticated("Actor must be registered", json!({})))?;

        let now = self.clock.now_utc();

        // 1. Persist override to DB.
        self.feature_flag_repository
            .upsert_override(flag, enabled, Some(actor_id), now)
            .await?;

        // 2. Update in-memory registry so the change is immediately visible.
        self.shared_flags.write().await.toggle(flag, enabled);

        // 3. Emit audit entry (failures are logged but do not undo the toggle).
        self.emit_audit(AdminAuditEntry {
            id: None,
            actor_user_id: actor.id,
            target_user_id: None,
            action_code: AuditActionCode::AdminFeatureToggled,
            metadata: json!({
                "flag_key": flag.as_key(),
                "enabled": enabled,
            }),
            created_at: now,
        })
        .await;

        Ok(enabled)
    }

    async fn emit_audit(&self, entry: AdminAuditEntry) {
        if let Err(error) = self.admin_audit_repository.append(&entry).await {
            warn!(
                target = "admin.audit",
                action = ?entry.action_code,
                target_user_id = entry.target_user_id,
                error = %error,
                "failed to persist admin audit entry"
            );
        }
    }
}
