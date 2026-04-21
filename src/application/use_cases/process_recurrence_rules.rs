//! Recurrence-rule scheduler (background use case).
//!
//! On every tick it:
//!
//! 1. Queries `recurrence_rules WHERE is_active = 1 AND next_run_at <= now`.
//! 2. For each due rule, loads the linked `task_templates` row and builds a
//!    new `Task` directly via [`TaskRepository::create_if_absent`].
//! 3. Advances the rule's schedule by writing `last_run_at = now` and
//!    recomputing `next_run_at` via [`CronExpression::next_fire_after`].
//!
//! Rules without a linked template are skipped with a warning log — a future
//! phase will support "raw body" rules where the task content is stored inline
//! on the rule itself.
//!
//! The use case is gated by the `recurrence_rules` feature flag and is a
//! complete no-op when the flag is disabled.

use std::sync::Arc;

use uuid::Uuid;

use crate::application::ports::repositories::{RecurrenceRepository, TaskRepository};
use crate::application::ports::services::Clock;
use crate::domain::errors::AppResult;
use crate::domain::recurrence::{CronExpression, RecurrenceRule};
use crate::domain::task::{MessageType, Task, TaskPriority, TaskStatus};
use crate::shared::feature_flags::{FeatureFlag, SharedFeatureFlagRegistry};

/// Maximum number of due rules processed in a single tick.  Keeps individual
/// ticks bounded even when a clock skew causes many rules to fire at once.
const MAX_RULES_PER_TICK: i64 = 50;

pub struct ProcessRecurrenceRulesUseCase {
    clock: Arc<dyn Clock>,
    recurrence_repo: Arc<dyn RecurrenceRepository>,
    task_repo: Arc<dyn TaskRepository>,
    feature_flags: SharedFeatureFlagRegistry,
}

impl ProcessRecurrenceRulesUseCase {
    pub fn new(
        clock: Arc<dyn Clock>,
        recurrence_repo: Arc<dyn RecurrenceRepository>,
        task_repo: Arc<dyn TaskRepository>,
        feature_flags: SharedFeatureFlagRegistry,
    ) -> Self {
        Self {
            clock,
            recurrence_repo,
            task_repo,
            feature_flags,
        }
    }

    pub async fn execute(&self) -> AppResult<()> {
        if !self
            .feature_flags
            .read()
            .await
            .is_enabled(FeatureFlag::RecurrenceRules)
        {
            return Ok(());
        }

        let now = self.clock.now_utc();
        let due_rules = self
            .recurrence_repo
            .list_due(now, MAX_RULES_PER_TICK)
            .await?;

        for rule in &due_rules {
            // Rules without a template are not yet supported in v1.
            let Some(template_id) = rule.template_id else {
                tracing::debug!(rule_id = rule.id, "recurrence_rule_has_no_template_skipped");
                self.advance(rule, now).await;
                continue;
            };

            let template = match self.recurrence_repo.get_template(template_id).await {
                Ok(Some(t)) => t,
                Ok(None) => {
                    tracing::warn!(
                        rule_id = rule.id,
                        template_id,
                        "recurrence_rule_template_missing_or_inactive"
                    );
                    self.advance(rule, now).await;
                    continue;
                }
                Err(err) => {
                    tracing::warn!(
                        rule_id = rule.id,
                        error = %err,
                        "recurrence_rule_template_load_failed"
                    );
                    continue; // Don't advance — retry on next tick.
                }
            };

            let body = match crate::domain::recurrence::decode_template_body(&template.body) {
                Ok(b) => b,
                Err(err) => {
                    tracing::warn!(
                        rule_id = rule.id,
                        error = %err,
                        "recurrence_rule_template_body_invalid"
                    );
                    self.advance(rule, now).await;
                    continue;
                }
            };

            // Build the task directly from the template.  `source_message_key`
            // encodes the rule + timestamp so `create_if_absent` deduplicates
            // if the worker restarts and re-processes the same tick.
            let source_key = format!("recurrence:{}:{}", rule.id, now.timestamp());
            let task = Task {
                id: None,
                task_uid: Uuid::now_v7(),
                version: 1,
                source_message_key: source_key,
                created_by_user_id: rule.owner_user_id,
                assigned_to_user_id: None,
                assigned_to_employee_id: None,
                title: template.title.clone(),
                description: body.description.clone(),
                acceptance_criteria: body.acceptance_criteria.clone(),
                expected_result: body.expected_result.clone(),
                deadline: None,
                deadline_raw: None,
                original_message: format!("Автозадача из шаблона «{}»", template.code),
                message_type: MessageType::Text,
                ai_model_used: String::new(),
                ai_response_raw: String::new(),
                status: TaskStatus::Created,
                priority: TaskPriority::Medium,
                blocked_reason: None,
                telegram_chat_id: 0,
                telegram_message_id: 0,
                telegram_task_message_id: None,
                tags: body.tags.clone(),
                created_at: now,
                sent_at: None,
                started_at: None,
                blocked_at: None,
                review_requested_at: None,
                completed_at: None,
                cancelled_at: None,
                updated_at: now,
            };

            match self.task_repo.create_if_absent(&task).await {
                Ok(_) => {
                    tracing::info!(
                        rule_id = rule.id,
                        template_code = %template.code,
                        "recurrence_task_created"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        rule_id = rule.id,
                        error = %err,
                        "recurrence_task_creation_failed"
                    );
                }
            }

            self.advance(rule, now).await;
        }

        Ok(())
    }

    /// Advance a rule's schedule.  Errors are soft-logged so a broken CRON
    /// string or unknown timezone does not prevent other rules from firing.
    async fn advance(
        &self,
        rule: &crate::application::ports::repositories::RecurrenceRuleRow,
        now: chrono::DateTime<chrono::Utc>,
    ) {
        let next_run_at = compute_next_run(&rule.cron_expression, &rule.timezone, now);
        if let Err(err) = self
            .recurrence_repo
            .advance_rule(rule.id, now, next_run_at, now)
            .await
        {
            tracing::warn!(
                rule_id = rule.id,
                error = %err,
                "recurrence_rule_advance_failed"
            );
        }
    }
}

/// Parses the CRON expression and timezone, then computes the next firing time
/// after `from`.  Returns `None` on any parse error so callers can store a
/// NULL `next_run_at` (the rule will be skipped on future ticks until an
/// operator fixes the expression).
fn compute_next_run(
    cron_expr: &str,
    tz_code: &str,
    from: chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let cron = CronExpression::parse(cron_expr)
        .map_err(|e| {
            tracing::warn!(cron = %cron_expr, error = %e, "recurrence_cron_parse_failed");
        })
        .ok()?;
    let tz = RecurrenceRule::parse_timezone(tz_code)
        .map_err(|e| {
            tracing::warn!(tz = %tz_code, error = %e, "recurrence_timezone_parse_failed");
        })
        .ok()?;
    cron.next_fire_after(from, tz)
}
