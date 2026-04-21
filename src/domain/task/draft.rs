use serde::{Deserialize, Serialize};
use serde_json::json;
use validator::Validate;

use crate::domain::errors::AppError;
use crate::domain::errors::AppResult;
use crate::shared::constants::limits::{
    MAX_ACCEPTANCE_CRITERIA, MAX_TASK_ACCEPTANCE_CRITERION_LENGTH, MAX_TASK_EXPECTED_RESULT_LENGTH,
    MAX_TASK_STEPS, MAX_TASK_STEP_LENGTH, MAX_TASK_TITLE_LENGTH, MIN_TASK_STEPS,
};

/// The structured AI-produced representation of a new task.
///
/// Validated by both the `validator` derive macro (schema-level invariants) and
/// [`validate_business_rules`](StructuredTaskDraft::validate_business_rules) (domain limits).
///
/// Hardening notes (P1-ai-prompt-hardening):
/// * `deadline_iso` is optional because the AI is explicitly permitted to
///   leave it empty when the user did not express a deadline.  When the
///   AI returns a value we *still* pass it through [`crate::domain::deadline::DeadlineResolver`],
///   which validates format, non-past-instant, and calendar rules.
/// * Every new optional field **must** use `#[serde(default)]` so responses
///   from older Gemini deployments don't break deserialisation during
///   rollouts.
#[derive(Debug, Clone, Serialize, Deserialize, Validate)]
pub struct StructuredTaskDraft {
    #[validate(length(min = 1, max = 100))]
    pub title: String,
    #[validate(length(min = 1))]
    pub expected_result: String,
    pub steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    /// ISO-8601 (RFC3339) or bare `YYYY-MM-DD` deadline suggested by the
    /// AI.  Callers **must not** trust it blindly — run through
    /// `DeadlineResolver::resolve` before persisting.
    #[serde(default)]
    pub deadline_iso: Option<String>,
    /// Explicit refusal marker.  When set to `true`, all other fields must
    /// be ignored and the task creation flow must fall back to the
    /// deterministic parser / ask-user path.  Used so the model can say
    /// "I'm not confident in this request" instead of fabricating a
    /// title.
    #[serde(default)]
    pub refused: bool,
    /// Free-form refusal reason echoed back to the user when
    /// `refused == true`.  Never longer than a single paragraph.
    #[serde(default)]
    pub refusal_reason: Option<String>,
}

impl StructuredTaskDraft {
    /// Validates all domain-level business rules beyond basic schema checks.
    ///
    /// Enforces step count, field length limits for Telegram delivery, and
    /// acceptance-criteria count so that task cards remain readable.
    pub fn validate_business_rules(&self) -> AppResult<()> {
        if self.refused {
            return Err(AppError::business_rule(
                "TASK_DRAFT_REFUSED",
                "AI refused to produce a task for this input",
                json!({ "reason": self.refusal_reason.clone() }),
            ));
        }

        self.validate().map_err(|error| {
            AppError::schema_validation(
                "TASK_DRAFT_INVALID",
                "AI response does not match the expected schema",
                json!({ "errors": error.to_string() }),
            )
        })?;

        if !(MIN_TASK_STEPS..=MAX_TASK_STEPS).contains(&self.steps.len()) {
            return Err(AppError::business_rule(
                "TASK_STEPS_INVALID",
                "Task must contain between 1 and 7 concrete steps",
                json!({ "count": self.steps.len() }),
            ));
        }

        if self.acceptance_criteria.len() > MAX_ACCEPTANCE_CRITERIA {
            return Err(AppError::business_rule(
                "TASK_ACCEPTANCE_CRITERIA_INVALID",
                "Task contains too many acceptance criteria",
                json!({ "count": self.acceptance_criteria.len() }),
            ));
        }

        if self.title.chars().count() > MAX_TASK_TITLE_LENGTH {
            return Err(AppError::business_rule(
                "TASK_TITLE_TOO_LONG",
                "Task title is too long",
                json!({ "limit": MAX_TASK_TITLE_LENGTH }),
            ));
        }

        if self.expected_result.chars().count() > MAX_TASK_EXPECTED_RESULT_LENGTH {
            return Err(AppError::business_rule(
                "TASK_EXPECTED_RESULT_TOO_LONG",
                "Task expected result is too long",
                json!({ "limit": MAX_TASK_EXPECTED_RESULT_LENGTH }),
            ));
        }

        if let Some(step_length) = self
            .steps
            .iter()
            .map(|value| value.chars().count())
            .find(|value| *value > MAX_TASK_STEP_LENGTH)
        {
            return Err(AppError::business_rule(
                "TASK_STEP_TOO_LONG",
                "Task contains a step that is too long for Telegram delivery",
                json!({ "limit": MAX_TASK_STEP_LENGTH, "length": step_length }),
            ));
        }

        if let Some(criterion_length) = self
            .acceptance_criteria
            .iter()
            .map(|value| value.chars().count())
            .find(|value| *value > MAX_TASK_ACCEPTANCE_CRITERION_LENGTH)
        {
            return Err(AppError::business_rule(
                "TASK_ACCEPTANCE_CRITERION_TOO_LONG",
                "Task contains an acceptance criterion that is too long for Telegram delivery",
                json!({
                    "limit": MAX_TASK_ACCEPTANCE_CRITERION_LENGTH,
                    "length": criterion_length,
                }),
            ));
        }

        Ok(())
    }
}
