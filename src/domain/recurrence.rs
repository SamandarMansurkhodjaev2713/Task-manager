//! Task-template & recurrence-rule value objects (Phase 11 skeleton).
//!
//! These types shadow `task_templates` and `recurrence_rules` (migration
//! 012).  The goal of this skeleton is NOT to ship a full recurrence
//! engine — that is a separate phase — but to lock in:
//!
//! 1. The canonical body format for templates, so the on-disk JSON does
//!    not drift between revisions.
//! 2. Validated wrappers around CRON strings and timezones, so repository
//!    adapters can never insert a rule that the scheduler cannot fire.
//!
//! The scheduler itself (background job) will be added in a later phase
//! that also wires the SQLite repository; until then we only expose pure
//! VOs for use in tests and admin tooling.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use cron::Schedule;
use serde::{Deserialize, Serialize};

use crate::domain::errors::AppError;

pub const MIN_TEMPLATE_CODE_LEN: usize = 2;
pub const MAX_TEMPLATE_CODE_LEN: usize = 32;
pub const MAX_TEMPLATE_TITLE_LEN: usize = 200;

/// A template's canonical payload.  Serialised verbatim into
/// `task_templates.body`.  Extra fields may be added over time; unknown
/// fields are preserved via `#[serde(flatten)]` so older binaries don't
/// lose data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTemplateBody {
    pub description: String,
    #[serde(default)]
    pub acceptance_criteria: Vec<String>,
    #[serde(default)]
    pub expected_result: String,
    #[serde(default)]
    pub tags: Vec<String>,
}

/// Task template metadata + body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskTemplate {
    pub id: Option<i64>,
    pub code: TemplateCode,
    pub title: String,
    pub body: TaskTemplateBody,
    pub created_by_user_id: Option<i64>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A validated slug-style identifier for a template.  Accepts ASCII
/// letters, digits, underscore and hyphen; everything else is rejected
/// so the code is safe to embed in callback data and URLs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemplateCode(String);

impl TemplateCode {
    pub fn parse(raw: &str) -> Result<Self, AppError> {
        let trimmed = raw.trim();
        let char_count = trimmed.chars().count();
        if !(MIN_TEMPLATE_CODE_LEN..=MAX_TEMPLATE_CODE_LEN).contains(&char_count) {
            return Err(AppError::schema_validation(
                "TEMPLATE_CODE_INVALID_LENGTH",
                format!(
                    "template code must have {MIN_TEMPLATE_CODE_LEN}..{MAX_TEMPLATE_CODE_LEN} characters"
                ),
                serde_json::json!({ "len": char_count }),
            ));
        }
        if !trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(AppError::schema_validation(
                "TEMPLATE_CODE_INVALID_CHARACTERS",
                "template code may only contain letters, digits, '-' and '_'",
                serde_json::json!({}),
            ));
        }
        Ok(Self(trimmed.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated CRON expression (5 or 6 fields, as accepted by `cron`).
///
/// Wraps both the canonical string form and a pre-parsed `Schedule` so
/// callers never have to re-parse at hot-path firing time.
#[derive(Debug, Clone)]
pub struct CronExpression {
    raw: String,
    schedule: Schedule,
}

impl PartialEq for CronExpression {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl Eq for CronExpression {}

impl CronExpression {
    pub fn parse(raw: &str) -> Result<Self, AppError> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(AppError::schema_validation(
                "CRON_EXPRESSION_EMPTY",
                "cron expression must not be empty",
                serde_json::json!({}),
            ));
        }
        let schedule = Schedule::from_str(trimmed).map_err(|error| {
            AppError::schema_validation(
                "CRON_EXPRESSION_INVALID",
                "cron expression is not a valid schedule",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;
        Ok(Self {
            raw: trimmed.to_owned(),
            schedule,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Next firing time at or after `from`, projected to `timezone`.
    pub fn next_fire_after(&self, from: DateTime<Utc>, timezone: Tz) -> Option<DateTime<Utc>> {
        let from_local = from.with_timezone(&timezone);
        self.schedule
            .after(&from_local)
            .next()
            .map(|next| next.with_timezone(&Utc))
    }
}

/// Validated recurrence rule.  Combines a CRON expression with the
/// timezone in which the expression should be evaluated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecurrenceRule {
    pub id: Option<i64>,
    pub template_id: Option<i64>,
    pub owner_user_id: i64,
    pub cron: CronExpression,
    pub timezone: Tz,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RecurrenceRule {
    pub fn parse_timezone(code: &str) -> Result<Tz, AppError> {
        code.parse::<Tz>().map_err(|_| {
            AppError::schema_validation(
                "RECURRENCE_RULE_UNKNOWN_TZ",
                format!("unknown timezone '{code}'"),
                serde_json::json!({ "timezone": code }),
            )
        })
    }

    /// Recomputes `next_run_at` from the cron expression, timezone and a
    /// reference moment (typically "now").  Returns a new `RecurrenceRule`
    /// without mutating `self`.
    pub fn with_refreshed_next_run(mut self, from: DateTime<Utc>) -> Self {
        self.next_run_at = self.cron.next_fire_after(from, self.timezone);
        self
    }
}

/// Encode a `TaskTemplateBody` into its canonical JSON form for storage.
pub fn encode_template_body(body: &TaskTemplateBody) -> Result<String, AppError> {
    serde_json::to_string(body).map_err(|error| {
        AppError::internal(
            "TEMPLATE_BODY_ENCODE_FAILED",
            "failed to encode template body as JSON",
            serde_json::json!({ "error": error.to_string() }),
        )
    })
}

/// Decode a `TaskTemplateBody` from its on-disk JSON form.
pub fn decode_template_body(raw: &str) -> Result<TaskTemplateBody, AppError> {
    serde_json::from_str(raw).map_err(|error| {
        AppError::schema_validation(
            "TEMPLATE_BODY_DECODE_FAILED",
            "failed to decode template body",
            serde_json::json!({ "error": error.to_string() }),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn given_valid_template_code_when_parsed_then_trimmed() {
        let code = TemplateCode::parse(" weekly-report ").expect("valid");
        assert_eq!(code.as_str(), "weekly-report");
    }

    #[test]
    fn given_template_code_with_invalid_chars_when_parsed_then_rejected() {
        let err = TemplateCode::parse("weekly report").unwrap_err();
        assert_eq!(err.code(), "TEMPLATE_CODE_INVALID_CHARACTERS");
    }

    #[test]
    fn given_short_template_code_when_parsed_then_rejected() {
        let err = TemplateCode::parse("a").unwrap_err();
        assert_eq!(err.code(), "TEMPLATE_CODE_INVALID_LENGTH");
    }

    #[test]
    fn given_valid_cron_when_parsed_then_fires_hourly() {
        use chrono::TimeZone;
        // `cron` crate expects 7 fields by default (sec min hour day-of-month month day-of-week year)
        let cron = CronExpression::parse("0 0 * * * * *").expect("valid");
        let tz = "Europe/Moscow".parse::<Tz>().expect("tz");
        let now = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 20, 12, 15, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);

        let next = cron.next_fire_after(now, tz).expect("next fire");
        let next_local = next.with_timezone(&tz);
        // Next "on the hour" after 12:15 is 13:00.
        use chrono::Timelike;
        assert_eq!(next_local.hour(), 13);
        assert_eq!(next_local.minute(), 0);
    }

    #[test]
    fn given_invalid_cron_when_parsed_then_rejected() {
        let err = CronExpression::parse("not a cron").unwrap_err();
        assert_eq!(err.code(), "CRON_EXPRESSION_INVALID");
    }

    #[test]
    fn given_template_body_when_roundtrip_encoded_then_fields_preserved() {
        let body = TaskTemplateBody {
            description: "Отправить отчёт".to_string(),
            acceptance_criteria: vec!["Отчёт на русском".to_string()],
            expected_result: "Email получателю".to_string(),
            tags: vec!["weekly".to_string()],
        };

        let encoded = encode_template_body(&body).expect("encode");
        let decoded = decode_template_body(&encoded).expect("decode");
        assert_eq!(body, decoded);
    }

    #[test]
    fn given_rule_when_refreshed_then_next_run_at_becomes_some() {
        let cron = CronExpression::parse("0 0 * * * * *").expect("valid");
        let tz = "Europe/Moscow".parse::<Tz>().expect("tz");
        let now = Utc::now();

        let rule = RecurrenceRule {
            id: None,
            template_id: None,
            owner_user_id: 1,
            cron,
            timezone: tz,
            next_run_at: None,
            last_run_at: None,
            is_active: true,
            created_at: now,
            updated_at: now,
        };

        let refreshed = rule.with_refreshed_next_run(now);
        assert!(refreshed.next_run_at.is_some());
    }
}
