//! SLA escalation worker (background use case).
//!
//! Scans every open task with a concrete deadline and computes its SLA state
//! based on calendar-day proximity.  When the state escalates (healthy →
//! at_risk → breached), it:
//!
//! 1. Writes the new `sla_state` and `sla_last_level` back to the task row.
//! 2. Inserts a row into `sla_escalations` (idempotent via `INSERT OR IGNORE`).
//! 3. Enqueues a [`NotificationType::SlaEscalation`] notification so the
//!    assigned user is notified on the next notification-processor tick.
//!
//! The entire execution is a **no-op when the `sla_escalations` feature flag
//! is disabled**, so toggling the flag in the admin panel takes effect on the
//! next scheduler tick (≤ 5 minutes) without requiring a restart.
//!
//! ## SLA thresholds (v1)
//!
//! * `at_risk`  — deadline is today or tomorrow (≤ 1 calendar day away).
//! * `breached` — deadline is in the past (< 0 days remaining).
//!
//! Full `WorkingCalendar`-aware arithmetic is deferred to a future phase once
//! the `working_calendars` table has operator-supplied data.

use std::sync::Arc;

use chrono::Duration;

use crate::application::ports::repositories::{NotificationRepository, SlaRepository};
use crate::application::ports::services::Clock;
use crate::domain::errors::AppResult;
use crate::domain::notification::{Notification, NotificationDeliveryState, NotificationType};
use crate::domain::sla::SlaState;
use crate::shared::feature_flags::{FeatureFlag, SharedFeatureFlagRegistry};

pub struct UpdateSlaStatesUseCase {
    clock: Arc<dyn Clock>,
    sla_repo: Arc<dyn SlaRepository>,
    notification_repo: Arc<dyn NotificationRepository>,
    feature_flags: SharedFeatureFlagRegistry,
}

impl UpdateSlaStatesUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        sla_repo: Arc<dyn SlaRepository>,
        notification_repo: Arc<dyn NotificationRepository>,
        feature_flags: SharedFeatureFlagRegistry,
    ) -> Self {
        Self {
            clock,
            sla_repo,
            notification_repo,
            feature_flags,
        }
    }

    pub async fn execute(&self) -> AppResult<()> {
        // Honour runtime feature gate — check on every tick so a runtime
        // toggle takes effect without a process restart.
        if !self
            .feature_flags
            .read()
            .await
            .is_enabled(FeatureFlag::SlaEscalations)
        {
            return Ok(());
        }

        let now = self.clock.now_utc();
        let today = self.clock.today_utc();
        let tasks = self.sla_repo.list_active_with_deadline(500).await?;

        for task in tasks {
            let days_remaining = (task.deadline - today).num_days();

            // Determine the new SLA state and escalation level.
            // Level 0 = healthy (no escalation), 1 = at_risk, 2 = breached.
            let (new_state, new_level) = if days_remaining < 0 {
                (SlaState::Breached, 2_i32)
            } else if days_remaining <= 1 {
                (SlaState::AtRisk, 1_i32)
            } else {
                (SlaState::Healthy, 0_i32)
            };

            let new_state_code = new_state.as_code();
            let already_at_state = task.current_sla_state.as_deref() == Some(new_state_code);
            let already_at_level = task.sla_last_level >= new_level;

            if already_at_state && already_at_level {
                // Nothing to do for this task.
                continue;
            }

            // Persist the updated state even when the level hasn't changed
            // (e.g. the state code column was NULL from migration and needs
            // to be back-filled).
            self.sla_repo
                .update_sla_state(task.id, new_state_code, new_level, now)
                .await?;

            // Only attempt escalation when the level is actually rising.
            if new_level <= task.sla_last_level || new_level == 0 {
                continue;
            }

            let detail = serde_json::json!({
                "days_remaining": days_remaining,
                "deadline": task.deadline.to_string(),
            });
            let is_new_escalation = self
                .sla_repo
                .record_escalation(task.id, new_level, "system", detail, now)
                .await?;

            if !is_new_escalation {
                // Already escalated at this level on a previous tick.
                continue;
            }

            // Only notify when there is a concrete assignee.
            let Some(recipient_user_id) = task.assigned_to_user_id else {
                continue;
            };

            let message = match new_state {
                SlaState::AtRisk => format!(
                    "⚠️ Задача «{}» приближается к дедлайну ({}).\nОстался {} дн. — проверьте статус.",
                    task.title,
                    task.deadline.format("%d.%m.%Y"),
                    days_remaining.max(0),
                ),
                SlaState::Breached => format!(
                    "🔴 Задача «{}» просрочена (дедлайн {} прошёл).\nСрочно обновите статус или сообщите ответственному.",
                    task.title,
                    task.deadline.format("%d.%m.%Y"),
                ),
                SlaState::Healthy => continue, // level 0 never reaches here
            };

            // `dedupe_key` ensures the notification is sent at most once per
            // (task, level) pair even if the worker restarts mid-run.
            let dedupe_key = format!("sla:{}:level:{}", task.task_uid, new_level);
            let notification = Notification {
                id: None,
                task_id: Some(task.id),
                recipient_user_id,
                notification_type: NotificationType::SlaEscalation,
                message,
                dedupe_key,
                telegram_message_id: None,
                delivery_state: NotificationDeliveryState::Pending,
                is_sent: false,
                is_read: false,
                attempt_count: 0,
                sent_at: None,
                read_at: None,
                next_attempt_at: Some(now + Duration::seconds(5)),
                last_error_code: None,
                created_at: now,
            };

            if let Err(err) = self.notification_repo.enqueue(&notification).await {
                tracing::warn!(
                    task_id = task.id,
                    level = new_level,
                    error = %err,
                    "sla_escalation_notification_enqueue_failed"
                );
            }
        }

        Ok(())
    }
}
