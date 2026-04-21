//! Unified deadline kernel.
//!
//! Every caller that turns "free text + optional AI hint + user context"
//! into a concrete due date MUST go through [`DeadlineResolver::resolve`].
//! There is deliberately a single function for the whole fleet because:
//!
//! * The P0 screenshot showed the bot persisting `"до пятницы"` as raw
//!   string with a separately parsed `NaiveDate` that ignored the user's
//!   timezone.  Two representations of the same deadline drift apart, and
//!   users see one version on the task card, another in reminders.
//! * AI-structured outputs (phase P1-ai-prompt-hardening) return
//!   `deadline_iso` in strict ISO-8601.  The kernel prioritises AI if the
//!   value validates, falls back to the deterministic regex parser, and
//!   never panics.
//! * Working calendar (`WorkingCalendar`) is consulted *after* we know a
//!   calendar date — the kernel clamps the wall-clock to the end of the
//!   working day in the user's timezone, so "завтра" does not mean
//!   "23:59 UTC".  Tasks that cross weekends are rolled to the next
//!   working day using the same SLA primitive that drives escalations,
//!   which keeps reminders and the `sla_state` calculation consistent.
//!
//! This module is pure: zero I/O, zero clock side effects.  The caller
//! feeds `now_utc`; unit tests use `chrono::Utc::now()` and integration
//! tests use `FrozenClock` from the infrastructure layer.

use chrono::{DateTime, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::domain::errors::AppResult;
use crate::domain::parsing;
use crate::domain::sla::WorkingCalendar;

/// Default end-of-business for a user-facing deadline.  18:00 local time is
/// our product default (matches `WorkingCalendar::new(..., 1080, ..)`).  The
/// kernel falls back to this when the AI hint lacks a time component and
/// the calendar does not otherwise constrain the answer.
pub const DEFAULT_END_OF_BUSINESS_MIN: u16 = 18 * 60;

/// Source of the final deadline.  Exposed on [`Deadline`] so the UI can
/// say "распознано ИИ" / "распознано по тексту" without guessing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeadlineSource {
    /// The AI structured output supplied a valid ISO-8601 timestamp.
    AiStructured,
    /// Regex-based fallback (sync, no network).
    DeterministicText,
    /// User explicitly left the deadline empty (e.g. tapped "Без срока").
    NoneProvided,
}

/// Resolved deadline, paired with the provenance that the UI wants to
/// display alongside the date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    /// UTC instant used for persistence, SLA, notifications.
    pub utc: Option<DateTime<Utc>>,
    /// Calendar date in the user's timezone — the one we display on the
    /// task card.  `None` if and only if `utc` is `None`.
    pub local_date: Option<NaiveDate>,
    /// Raw fragment extracted from the user input (useful for reminders
    /// that want to quote the user back to themselves, e.g.
    /// "вы написали «до пятницы»").
    pub raw_fragment: Option<String>,
    /// Where the deadline came from — see [`DeadlineSource`].
    pub source: DeadlineSource,
    /// Confidence in the range 0..=100; 100 means the AI returned a
    /// fully-qualified timestamp, 80 means regex matched with day+month,
    /// 50 means regex matched a weekday only.
    pub confidence: u8,
}

impl Deadline {
    pub fn none() -> Self {
        Self {
            utc: None,
            local_date: None,
            raw_fragment: None,
            source: DeadlineSource::NoneProvided,
            confidence: 0,
        }
    }

    pub fn has_value(&self) -> bool {
        self.utc.is_some()
    }

    /// Formatted local date for inline UI labels (`"до 24.04.2026"`).
    pub fn local_label(&self) -> Option<String> {
        self.local_date
            .map(|date| date.format("%d.%m.%Y").to_string())
    }
}

/// Input envelope: everything the kernel needs in order to be pure.
#[derive(Debug, Clone)]
pub struct DeadlineInput<'a> {
    /// The user's own text (voice transcript or typed).
    pub text: &'a str,
    /// Optional ISO-8601 timestamp produced by the AI step.  If set, it
    /// wins when it parses and is not in the past.
    pub ai_iso_hint: Option<&'a str>,
    /// The user's timezone.  Must be a valid IANA name; the config layer
    /// rejects invalid strings before we get here.
    pub user_timezone: Tz,
    /// The current UTC instant.  Passed in so tests can freeze the clock.
    pub now_utc: DateTime<Utc>,
    /// Optional working calendar.  When present we clamp the final instant
    /// to `workday_end_min` on the chosen day and roll forward if that
    /// date is a non-working day.
    pub calendar: Option<&'a WorkingCalendar>,
}

/// Pure deadline kernel.
///
/// Use [`DeadlineResolver::resolve`] — the struct has no state, it exists
/// only to group the API under a single name.
pub struct DeadlineResolver;

impl DeadlineResolver {
    pub fn resolve(input: DeadlineInput<'_>) -> AppResult<Deadline> {
        if let Some(hint) = input.ai_iso_hint {
            if let Some(resolved) = Self::try_ai_iso(hint, &input)? {
                return Ok(resolved);
            }
        }
        if let Some(resolved) = Self::try_deterministic(&input)? {
            return Ok(resolved);
        }
        Ok(Deadline::none())
    }

    fn try_ai_iso(hint: &str, input: &DeadlineInput<'_>) -> AppResult<Option<Deadline>> {
        let trimmed = hint.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }
        // Accept either a full RFC3339 timestamp or a bare YYYY-MM-DD.
        let parsed_utc = if let Ok(instant) = DateTime::parse_from_rfc3339(trimmed) {
            instant.with_timezone(&Utc)
        } else if let Ok(date) = NaiveDate::parse_from_str(trimmed, "%Y-%m-%d") {
            end_of_business_utc(date, input.user_timezone, input.calendar)
        } else {
            return Ok(None);
        };

        // Reject values that sit in the past relative to `now_utc` — the AI
        // occasionally hallucinates last-week dates when the user wrote
        // something ambiguous.  Fall back to the regex path instead of
        // persisting a stale deadline.
        if parsed_utc < input.now_utc {
            return Ok(None);
        }

        let clamped = apply_calendar(parsed_utc, input);
        let local_date = clamped.with_timezone(&input.user_timezone).date_naive();

        Ok(Some(Deadline {
            utc: Some(clamped),
            local_date: Some(local_date),
            raw_fragment: Some(trimmed.to_owned()),
            source: DeadlineSource::AiStructured,
            confidence: 100,
        }))
    }

    fn try_deterministic(input: &DeadlineInput<'_>) -> AppResult<Option<Deadline>> {
        let today_local = input
            .now_utc
            .with_timezone(&input.user_timezone)
            .date_naive();
        // Dedicated deadline-only extractor — does *not* run the
        // description-length validation of `parse_task_request`, so short
        // phrases like "до пятницы" still yield a deadline when the caller
        // is asking "did the user express any due date?" outside the
        // create-task path.
        let (date_opt, raw_opt) = parsing::extract_deadline_from_text(input.text, today_local)?;

        let Some(date) = date_opt else {
            return Ok(None);
        };
        let utc = apply_calendar(
            end_of_business_utc(date, input.user_timezone, input.calendar),
            input,
        );
        let local_date = utc.with_timezone(&input.user_timezone).date_naive();
        let confidence = deterministic_confidence(raw_opt.as_deref());

        Ok(Some(Deadline {
            utc: Some(utc),
            local_date: Some(local_date),
            raw_fragment: raw_opt,
            source: DeadlineSource::DeterministicText,
            confidence,
        }))
    }
}

fn end_of_business_utc(
    date: NaiveDate,
    timezone: Tz,
    calendar: Option<&WorkingCalendar>,
) -> DateTime<Utc> {
    let end_min = calendar
        .map(|c| c.workday_end_min())
        .unwrap_or(DEFAULT_END_OF_BUSINESS_MIN) as u32;
    let hour = end_min / 60;
    let minute = end_min % 60;
    let naive = date
        .and_hms_opt(hour, minute, 0)
        .expect("hour/minute derived from workday_end_min which is bounded");
    timezone
        .from_local_datetime(&naive)
        .single()
        .unwrap_or_else(|| timezone.from_utc_datetime(&naive))
        .with_timezone(&Utc)
}

fn apply_calendar(candidate: DateTime<Utc>, input: &DeadlineInput<'_>) -> DateTime<Utc> {
    let Some(calendar) = input.calendar else {
        return candidate;
    };
    let local = candidate.with_timezone(&calendar.timezone());
    let day = local.date_naive();
    if calendar.is_working_day(day) {
        return candidate;
    }
    // Roll forward to the next working day; clamp to workday_end_min.
    for offset in 1..=366_i64 {
        let candidate_day = day + Duration::days(offset);
        if calendar.is_working_day(candidate_day) {
            return end_of_business_utc(candidate_day, calendar.timezone(), Some(calendar));
        }
    }
    candidate
}

fn deterministic_confidence(raw: Option<&str>) -> u8 {
    match raw {
        Some(value) if value.contains('.') || value.contains('/') => 90,
        Some(value)
            if matches!(
                value.to_lowercase().as_str(),
                "сегодня" | "завтра" | "послезавтра"
            ) =>
        {
            85
        }
        Some(value) if !value.is_empty() => {
            // weekday or "через N дней"
            if value.to_lowercase().starts_with("через") {
                75
            } else {
                60
            }
        }
        _ => 50,
    }
}

#[cfg(test)]
pub fn is_past_hint(hint: &str, now_utc: DateTime<Utc>, tz: Tz) -> bool {
    // Exposed for white-box testing of the stale-deadline guardrail.
    if let Ok(instant) = DateTime::parse_from_rfc3339(hint) {
        return instant.with_timezone(&Utc) < now_utc;
    }
    if let Ok(date) = NaiveDate::parse_from_str(hint, "%Y-%m-%d") {
        return end_of_business_utc(date, tz, None) < now_utc;
    }
    false
}

// Ensure today's date is preserved when converting between timezones so
// that callers do not accidentally land on the previous day because of a
// timezone shift.  Kept for future use when we refactor the daily summary
// trigger to re-use the same function.
#[allow(dead_code)]
fn same_local_day(a: DateTime<Utc>, b: DateTime<Utc>, tz: Tz) -> bool {
    a.with_timezone(&tz).date_naive() == b.with_timezone(&tz).date_naive()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};
    use chrono_tz::Europe::Moscow;

    fn moscow_noon(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
        Moscow
            .with_ymd_and_hms(y, m, d, h, 0, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc)
    }

    fn moscow_calendar() -> WorkingCalendar {
        WorkingCalendar::new(
            "RU_STANDARD",
            Moscow,
            (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4),
            540,
            1080,
            Vec::<NaiveDate>::new(),
            Vec::<NaiveDate>::new(),
        )
        .expect("valid calendar")
    }

    #[test]
    fn given_valid_ai_iso_timestamp_when_resolving_then_ai_wins() {
        let now = moscow_noon(2026, 4, 20, 12); // Monday
        let hint = "2026-04-24T15:00:00+03:00"; // Friday 15:00 Moscow

        let resolved = DeadlineResolver::resolve(DeadlineInput {
            text: "отчёт до пятницы",
            ai_iso_hint: Some(hint),
            user_timezone: Moscow,
            now_utc: now,
            calendar: None,
        })
        .expect("resolves");

        assert_eq!(resolved.source, DeadlineSource::AiStructured);
        assert_eq!(resolved.confidence, 100);
        let local = resolved.utc.unwrap().with_timezone(&Moscow);
        assert_eq!(local.hour(), 15);
        assert_eq!(local.day(), 24);
    }

    #[test]
    fn given_ai_iso_in_past_when_resolving_then_falls_back_to_text() {
        let now = moscow_noon(2026, 4, 20, 12); // Monday
        let resolved = DeadlineResolver::resolve(DeadlineInput {
            text: "сделать завтра",
            ai_iso_hint: Some("2020-01-01T00:00:00Z"),
            user_timezone: Moscow,
            now_utc: now,
            calendar: None,
        })
        .expect("resolves");

        assert_eq!(resolved.source, DeadlineSource::DeterministicText);
        let local = resolved.utc.unwrap().with_timezone(&Moscow);
        assert_eq!(local.day(), 21); // tomorrow
        assert_eq!(local.hour(), 18);
    }

    #[test]
    fn given_no_hint_and_no_text_deadline_when_resolving_then_none() {
        let now = moscow_noon(2026, 4, 20, 12);
        let resolved = DeadlineResolver::resolve(DeadlineInput {
            text: "подготовить релизный чек-лист",
            ai_iso_hint: None,
            user_timezone: Moscow,
            now_utc: now,
            calendar: None,
        })
        .expect("resolves");

        assert_eq!(resolved.source, DeadlineSource::NoneProvided);
        assert!(!resolved.has_value());
    }

    #[test]
    fn given_weekend_date_with_calendar_when_resolving_then_rolls_to_next_working_day() {
        let now = moscow_noon(2026, 4, 20, 12); // Monday 20 Apr 2026
        let calendar = moscow_calendar();

        // ISO for Saturday 25 Apr 2026 — non-working day.
        let resolved = DeadlineResolver::resolve(DeadlineInput {
            text: "",
            ai_iso_hint: Some("2026-04-25"),
            user_timezone: Moscow,
            now_utc: now,
            calendar: Some(&calendar),
        })
        .expect("resolves");

        let local = resolved.utc.unwrap().with_timezone(&Moscow);
        assert_eq!(local.weekday(), chrono::Weekday::Mon, "got: {local}");
        assert_eq!(local.hour(), 18);
    }

    #[test]
    fn given_date_only_iso_hint_when_resolving_then_clamped_to_end_of_business() {
        let now = moscow_noon(2026, 4, 20, 6);
        let resolved = DeadlineResolver::resolve(DeadlineInput {
            text: "",
            ai_iso_hint: Some("2026-04-22"),
            user_timezone: Moscow,
            now_utc: now,
            calendar: None,
        })
        .expect("resolves");

        let local = resolved.utc.unwrap().with_timezone(&Moscow);
        assert_eq!(local.day(), 22);
        assert_eq!(local.hour(), 18);
        assert_eq!(local.minute(), 0);
    }

    #[test]
    fn given_garbled_ai_hint_when_resolving_then_falls_back_to_text() {
        let now = moscow_noon(2026, 4, 20, 12);
        let resolved = DeadlineResolver::resolve(DeadlineInput {
            text: "отчёт завтра",
            ai_iso_hint: Some("not-a-date"),
            user_timezone: Moscow,
            now_utc: now,
            calendar: None,
        })
        .expect("resolves");

        assert_eq!(resolved.source, DeadlineSource::DeterministicText);
        assert!(resolved.local_label().unwrap().starts_with("21."));
    }

    #[test]
    fn given_weekday_phrase_when_resolving_then_confidence_is_at_least_sixty() {
        let now = moscow_noon(2026, 4, 20, 12);
        let resolved = DeadlineResolver::resolve(DeadlineInput {
            text: "до пятницы",
            ai_iso_hint: None,
            user_timezone: Moscow,
            now_utc: now,
            calendar: None,
        })
        .expect("resolves");

        assert!(resolved.has_value());
        assert!(resolved.confidence >= 60);
    }

    #[test]
    fn given_past_iso_hint_when_checking_then_is_past_hint_returns_true() {
        let now = moscow_noon(2026, 4, 20, 12);
        assert!(is_past_hint("2020-01-01", now, Moscow));
        assert!(!is_past_hint("2027-01-01", now, Moscow));
    }
}
