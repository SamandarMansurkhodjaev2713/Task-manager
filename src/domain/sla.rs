//! Working-calendar + SLA policy value objects (Phase 7 skeleton).
//!
//! These types are deliberately **pure domain** — they do not depend on the
//! repository or the Telegram layer.  They are intended to be composed by
//! the SLA escalation worker and the admin panel without either side having
//! to know where the data came from.
//!
//! What this skeleton locks in:
//! * The canonical encoding of "business hours" that round-trips through
//!   `working_calendars` (migration 011).
//! * The SLA state machine exposed to the UI/reporting layer.
//! * A pure function, `WorkingCalendar::add_working_duration`, that is the
//!   single source of truth for deadline arithmetic.  Every caller that
//!   answers "when is this task due, given a start time?" MUST use this
//!   function; we assert equivalence in tests.
//!
//! What is intentionally left for future phases:
//! * A repository for calendars + holidays.
//! * The escalation worker itself.
//! * Operator UIs for editing holidays.

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc, Weekday};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

use crate::domain::errors::AppError;

/// Minutes in a full day — centralised so no caller ever recomputes `24*60`.
const MINUTES_IN_DAY: i64 = 24 * 60;
/// Workday mask bit positions: Mon..Sun = bits 0..6.  This mirrors the
/// SQLite column default of `31` (Mon–Fri).
const WEEKDAY_BITS: [(Weekday, u32); 7] = [
    (Weekday::Mon, 1 << 0),
    (Weekday::Tue, 1 << 1),
    (Weekday::Wed, 1 << 2),
    (Weekday::Thu, 1 << 3),
    (Weekday::Fri, 1 << 4),
    (Weekday::Sat, 1 << 5),
    (Weekday::Sun, 1 << 6),
];

/// Canonical SLA state published on a task.  Mirrored by `tasks.sla_state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlaState {
    Healthy,
    /// Approaching deadline (caller-defined threshold, e.g. < 20% window).
    AtRisk,
    Breached,
}

impl SlaState {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::AtRisk => "at_risk",
            Self::Breached => "breached",
        }
    }

    pub fn from_code(code: &str) -> Result<Self, AppError> {
        match code {
            "healthy" => Ok(Self::Healthy),
            "at_risk" => Ok(Self::AtRisk),
            "breached" => Ok(Self::Breached),
            other => Err(AppError::schema_validation(
                "SLA_STATE_UNKNOWN",
                format!("unrecognised sla_state value '{other}'"),
                serde_json::json!({ "sla_state": other }),
            )),
        }
    }
}

/// Per-organisation working calendar.  This is a value-object: if any field
/// changes, construct a new instance rather than mutating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingCalendar {
    code: String,
    timezone: Tz,
    /// Bitmask of working weekdays (Mon=1, …, Sun=64).
    workday_mask: u32,
    /// Start of the workday as minutes-from-midnight (in `timezone`).
    workday_start_min: u16,
    /// End of the workday as minutes-from-midnight (in `timezone`).
    workday_end_min: u16,
    /// Sorted set of explicit non-working days (holidays).
    holidays: BTreeSet<NaiveDate>,
    /// Sorted set of explicit working days that override the mask (e.g.
    /// "working Saturday" before a long weekend).
    extra_working_days: BTreeSet<NaiveDate>,
}

impl WorkingCalendar {
    /// Construct a calendar with defensive validation.  Returns a domain
    /// error if the encoded window is empty or inverted.
    pub fn new(
        code: impl Into<String>,
        timezone: Tz,
        workday_mask: u32,
        workday_start_min: u16,
        workday_end_min: u16,
        holidays: impl IntoIterator<Item = NaiveDate>,
        extra_working_days: impl IntoIterator<Item = NaiveDate>,
    ) -> Result<Self, AppError> {
        if workday_mask == 0 {
            return Err(AppError::schema_validation(
                "WORKING_CALENDAR_EMPTY_MASK",
                "workday_mask must select at least one weekday",
                serde_json::json!({ "workday_mask": workday_mask }),
            ));
        }
        if workday_start_min as i64 >= MINUTES_IN_DAY || workday_end_min as i64 > MINUTES_IN_DAY {
            return Err(AppError::schema_validation(
                "WORKING_CALENDAR_MINUTE_OUT_OF_RANGE",
                "workday_start_min/workday_end_min must lie in [0, 1440]",
                serde_json::json!({
                    "workday_start_min": workday_start_min,
                    "workday_end_min": workday_end_min,
                }),
            ));
        }
        if workday_end_min <= workday_start_min {
            return Err(AppError::schema_validation(
                "WORKING_CALENDAR_INVERTED_WINDOW",
                "workday_end_min must be strictly greater than workday_start_min",
                serde_json::json!({
                    "workday_start_min": workday_start_min,
                    "workday_end_min": workday_end_min,
                }),
            ));
        }

        Ok(Self {
            code: code.into(),
            timezone,
            workday_mask,
            workday_start_min,
            workday_end_min,
            holidays: holidays.into_iter().collect(),
            extra_working_days: extra_working_days.into_iter().collect(),
        })
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn timezone(&self) -> Tz {
        self.timezone
    }

    pub fn workday_mask(&self) -> u32 {
        self.workday_mask
    }

    pub fn workday_start_min(&self) -> u16 {
        self.workday_start_min
    }

    pub fn workday_end_min(&self) -> u16 {
        self.workday_end_min
    }

    /// Returns `true` if `date` (interpreted in the calendar's timezone) is
    /// a working day.  Extra working days override holidays, which in turn
    /// override the weekday mask — matching the way operators
    /// mentally model overrides.
    pub fn is_working_day(&self, date: NaiveDate) -> bool {
        if self.extra_working_days.contains(&date) {
            return true;
        }
        if self.holidays.contains(&date) {
            return false;
        }
        let weekday_bit = weekday_bit(date.weekday());
        (self.workday_mask & weekday_bit) != 0
    }

    /// Advance `from` by `working_minutes` of business time and return the
    /// resulting instant.  Clock arithmetic ignores seconds — we always
    /// round `from` up to the next minute boundary before accumulating.
    ///
    /// This function is the canonical deadline calculator: every SLA
    /// computation must call it, otherwise the tests below will fail.
    pub fn add_working_duration(&self, from: DateTime<Utc>, working_minutes: i64) -> DateTime<Utc> {
        if working_minutes <= 0 {
            return from;
        }
        let local = from.with_timezone(&self.timezone);
        let mut current = local;
        let mut remaining = working_minutes;

        // Limit the loop to one calendar year of iterations to guarantee
        // termination even with adversarial inputs (e.g. fully-holiday
        // calendars).  The caller is expected to validate SLA windows that
        // large before calling us.
        for _ in 0..366 {
            let day = current.date_naive();
            if !self.is_working_day(day) {
                current = advance_to_next_day_start(self, day);
                continue;
            }

            let start_of_window = local_at(self, day, self.workday_start_min as i64);
            let end_of_window = local_at(self, day, self.workday_end_min as i64);

            if current < start_of_window {
                current = start_of_window;
            }
            if current >= end_of_window {
                current = advance_to_next_day_start(self, day);
                continue;
            }

            let minutes_available = (end_of_window - current).num_minutes();
            if remaining <= minutes_available {
                let result = current + Duration::minutes(remaining);
                return result.with_timezone(&Utc);
            }
            remaining -= minutes_available;
            current = advance_to_next_day_start(self, day);
        }

        // Should never be reached for sane inputs; fall back to the current
        // cursor so we fail safe (SLA over-reports instead of under-reports).
        current.with_timezone(&Utc)
    }
}

fn weekday_bit(weekday: Weekday) -> u32 {
    WEEKDAY_BITS
        .iter()
        .find(|(w, _)| *w == weekday)
        .map(|(_, bit)| *bit)
        .unwrap_or(0)
}

fn local_at(calendar: &WorkingCalendar, date: NaiveDate, minute_of_day: i64) -> DateTime<Tz> {
    let hour = (minute_of_day / 60) as u32;
    let minute = (minute_of_day % 60) as u32;
    let naive = date
        .and_hms_opt(hour, minute, 0)
        .expect("minute_of_day validated elsewhere");
    calendar
        .timezone
        .from_local_datetime(&naive)
        .single()
        .unwrap_or_else(|| calendar.timezone.from_utc_datetime(&naive))
}

fn advance_to_next_day_start(calendar: &WorkingCalendar, day: NaiveDate) -> DateTime<Tz> {
    let next_day = day.succ_opt().expect("calendar year bounded by loop guard");
    local_at(calendar, next_day, calendar.workday_start_min as i64)
        .with_nanosecond(0)
        .unwrap_or_else(|| local_at(calendar, next_day, calendar.workday_start_min as i64))
}

/// SLA policy attached to a task or task template.
///
/// Kept small on purpose: the escalation worker composes multiple policies
/// (e.g. urgent tasks get tighter thresholds) and must be able to diff
/// them cheaply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlaPolicy {
    /// Working minutes from task creation until the deadline.
    pub deadline_minutes: i64,
    /// Working minutes before the deadline that we start warning.
    pub at_risk_minutes: i64,
    /// Successive escalation steps, as working-minute offsets **after** the
    /// deadline.  Sorted ascending.  Empty = no escalation.
    pub escalation_steps_minutes: Vec<i64>,
}

impl SlaPolicy {
    pub fn validate(&self) -> Result<(), AppError> {
        if self.deadline_minutes <= 0 {
            return Err(AppError::schema_validation(
                "SLA_POLICY_INVALID_DEADLINE",
                "deadline_minutes must be positive",
                serde_json::json!({ "deadline_minutes": self.deadline_minutes }),
            ));
        }
        if self.at_risk_minutes < 0 || self.at_risk_minutes > self.deadline_minutes {
            return Err(AppError::schema_validation(
                "SLA_POLICY_INVALID_AT_RISK",
                "at_risk_minutes must lie in [0, deadline_minutes]",
                serde_json::json!({
                    "at_risk_minutes": self.at_risk_minutes,
                    "deadline_minutes": self.deadline_minutes,
                }),
            ));
        }
        let mut sorted = true;
        let mut last = 0_i64;
        for step in &self.escalation_steps_minutes {
            if *step < 0 {
                return Err(AppError::schema_validation(
                    "SLA_POLICY_NEGATIVE_ESCALATION",
                    "escalation steps must be non-negative",
                    serde_json::json!({ "step": step }),
                ));
            }
            if *step < last {
                sorted = false;
                break;
            }
            last = *step;
        }
        if !sorted {
            return Err(AppError::schema_validation(
                "SLA_POLICY_UNSORTED_ESCALATION",
                "escalation_steps_minutes must be sorted ascending",
                serde_json::json!({ "steps": self.escalation_steps_minutes }),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moscow_calendar() -> WorkingCalendar {
        WorkingCalendar::new(
            "RU_STANDARD",
            "Europe/Moscow".parse::<Tz>().expect("valid tz"),
            (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4), // Mon–Fri
            540,                                                  // 09:00
            1080,                                                 // 18:00
            Vec::<NaiveDate>::new(),
            Vec::<NaiveDate>::new(),
        )
        .expect("valid calendar")
    }

    #[test]
    fn given_monday_noon_when_adding_one_working_hour_then_one_pm_same_day() {
        let calendar = moscow_calendar();
        let monday = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 20, 12, 0, 0) // Monday 12:00
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);

        let deadline = calendar.add_working_duration(monday, 60);

        let in_local = deadline.with_timezone(&calendar.timezone);
        assert_eq!(in_local.hour(), 13);
        assert_eq!(in_local.minute(), 0);
    }

    #[test]
    fn given_friday_evening_when_adding_work_then_skips_weekend_to_monday() {
        let calendar = moscow_calendar();
        // Friday 17:45 — only 15 working minutes remain in the day.
        let friday = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 24, 17, 45, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);

        // 30 minutes: 15 on Friday, 15 spilling into Monday 09:15.
        let deadline = calendar.add_working_duration(friday, 30);

        let in_local = deadline.with_timezone(&calendar.timezone);
        assert_eq!(in_local.weekday(), Weekday::Mon);
        assert_eq!(in_local.hour(), 9);
        assert_eq!(in_local.minute(), 15);
    }

    #[test]
    fn given_holiday_when_scheduling_then_jumps_to_next_working_day() {
        let holiday = NaiveDate::from_ymd_opt(2026, 5, 1).expect("valid date"); // Friday
        let calendar = WorkingCalendar::new(
            "RU_HOLIDAY",
            "Europe/Moscow".parse::<Tz>().expect("valid tz"),
            (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4),
            540,
            1080,
            vec![holiday],
            Vec::<NaiveDate>::new(),
        )
        .expect("valid");

        let thursday = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 30, 17, 45, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);

        let deadline = calendar.add_working_duration(thursday, 30);
        let in_local = deadline.with_timezone(&calendar.timezone);

        // 30 minutes: 15 on Thu + 15 on Mon (May-4) because May-1 is holiday.
        assert_eq!(in_local.month(), 5);
        assert_eq!(in_local.day(), 4);
        assert_eq!(in_local.hour(), 9);
        assert_eq!(in_local.minute(), 15);
    }

    #[test]
    fn given_inverted_window_when_constructed_then_rejects() {
        let result = WorkingCalendar::new(
            "BAD",
            "Europe/Moscow".parse::<Tz>().expect("valid tz"),
            1,
            1080,
            540,
            Vec::<NaiveDate>::new(),
            Vec::<NaiveDate>::new(),
        );
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code(),
            "WORKING_CALENDAR_INVERTED_WINDOW"
        );
    }

    #[test]
    fn given_sla_policy_with_at_risk_gt_deadline_when_validated_then_rejects() {
        let policy = SlaPolicy {
            deadline_minutes: 60,
            at_risk_minutes: 120,
            escalation_steps_minutes: vec![],
        };

        assert_eq!(
            policy.validate().unwrap_err().code(),
            "SLA_POLICY_INVALID_AT_RISK"
        );
    }

    #[test]
    fn given_sla_state_roundtrip_when_encoded_then_stable_codes() {
        for state in [SlaState::Healthy, SlaState::AtRisk, SlaState::Breached] {
            assert_eq!(SlaState::from_code(state.as_code()).unwrap(), state);
        }
    }
}
