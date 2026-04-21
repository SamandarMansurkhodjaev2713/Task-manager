//! Per-user notification preferences value object (Phase 8 skeleton).
//!
//! Centralises quiet-hours + timezone arithmetic so the notification
//! dispatcher never needs to reach into the `User` struct directly.  The
//! dispatcher will call `NotificationPreferences::is_in_quiet_hours(now)`
//! and `next_deliverable_at(now)` to decide whether to send or defer a
//! message.
//!
//! Rules encoded here (locked down by unit tests below):
//! * Quiet hours are expressed as minutes-from-midnight in the user's local
//!   timezone (NOT UTC).  That keeps the user-visible semantics intuitive
//!   ("I don't want pings after 22:00 wherever I am").
//! * A start == end window means "quiet hours disabled".  Callers that
//!   don't want this default must explicitly check for it.
//! * Wrap-around windows (start > end, e.g. 22:00 → 08:00) are supported
//!   natively; this is in fact the default.

use chrono::{DateTime, TimeZone, Timelike, Utc};
use chrono_tz::Tz;

use crate::domain::errors::AppError;

const MINUTES_IN_DAY: i32 = 24 * 60;

/// A user's delivery preferences, extracted from the `users` columns
/// `timezone`, `quiet_hours_start_min`, and `quiet_hours_end_min`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPreferences {
    timezone: Tz,
    quiet_start_min: i32,
    quiet_end_min: i32,
}

impl NotificationPreferences {
    /// Construct with validation.  Returns a domain error if the timezone is
    /// unknown or either minute value is out of range.
    pub fn new(
        timezone_code: &str,
        quiet_start_min: i32,
        quiet_end_min: i32,
    ) -> Result<Self, AppError> {
        let timezone = timezone_code.parse::<Tz>().map_err(|_| {
            AppError::schema_validation(
                "NOTIFICATION_PREFERENCES_UNKNOWN_TZ",
                format!("unknown timezone '{timezone_code}'"),
                serde_json::json!({ "timezone": timezone_code }),
            )
        })?;

        if !in_minute_range(quiet_start_min) || !in_minute_range(quiet_end_min) {
            return Err(AppError::schema_validation(
                "NOTIFICATION_PREFERENCES_MINUTE_OUT_OF_RANGE",
                "quiet hours must lie in [0, 1440)",
                serde_json::json!({
                    "quiet_start_min": quiet_start_min,
                    "quiet_end_min": quiet_end_min,
                }),
            ));
        }

        Ok(Self {
            timezone,
            quiet_start_min,
            quiet_end_min,
        })
    }

    pub fn timezone(&self) -> Tz {
        self.timezone
    }

    pub fn quiet_start_min(&self) -> i32 {
        self.quiet_start_min
    }

    pub fn quiet_end_min(&self) -> i32 {
        self.quiet_end_min
    }

    pub fn quiet_hours_disabled(&self) -> bool {
        self.quiet_start_min == self.quiet_end_min
    }

    /// `true` iff `now` (UTC) falls inside the user's quiet-hours window
    /// when projected to their local timezone.
    pub fn is_in_quiet_hours(&self, now: DateTime<Utc>) -> bool {
        if self.quiet_hours_disabled() {
            return false;
        }
        let local = now.with_timezone(&self.timezone);
        let minute_of_day = (local.hour() as i32) * 60 + (local.minute() as i32);
        inside_window(minute_of_day, self.quiet_start_min, self.quiet_end_min)
    }

    /// Returns the earliest instant at or after `now` at which the user's
    /// quiet hours are NOT in effect.  If quiet hours are disabled, returns
    /// `now` unchanged.  If `now` is already outside the window, returns
    /// `now` unchanged as well.
    pub fn next_deliverable_at(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        if !self.is_in_quiet_hours(now) {
            return now;
        }
        let local = now.with_timezone(&self.timezone);
        let current_minute = (local.hour() as i32) * 60 + (local.minute() as i32);
        let minutes_to_advance =
            minutes_until_end_of_window(current_minute, self.quiet_start_min, self.quiet_end_min);
        let candidate_minute_of_day =
            (current_minute + minutes_to_advance).rem_euclid(MINUTES_IN_DAY);

        // Resolve to the concrete next occurrence:
        let mut date_local = local.date_naive();
        // If the window wraps past midnight and we need minutes-today > 1440 - now,
        // nudge `date_local` forward by one day.
        if current_minute + minutes_to_advance >= MINUTES_IN_DAY {
            date_local = date_local
                .succ_opt()
                .expect("succ is safe within chrono's supported range");
        }
        let hour = (candidate_minute_of_day / 60) as u32;
        let minute = (candidate_minute_of_day % 60) as u32;
        let naive = date_local
            .and_hms_opt(hour, minute, 0)
            .expect("hour/minute validated");
        let next_local = self
            .timezone
            .from_local_datetime(&naive)
            .single()
            .unwrap_or_else(|| self.timezone.from_utc_datetime(&naive));
        next_local.with_timezone(&Utc)
    }
}

fn in_minute_range(value: i32) -> bool {
    (0..MINUTES_IN_DAY).contains(&value)
}

fn inside_window(minute_of_day: i32, start: i32, end: i32) -> bool {
    if start < end {
        (start..end).contains(&minute_of_day)
    } else {
        // Wrap-around window (e.g. 22:00 → 08:00).
        minute_of_day >= start || minute_of_day < end
    }
}

fn minutes_until_end_of_window(current_minute: i32, start: i32, end: i32) -> i32 {
    if start < end {
        // Non-wrapping window — `end` is later today.
        (end - current_minute).max(1)
    } else if current_minute >= start {
        // Wrap-around, still on the "start-side" of midnight.
        // Minutes until midnight + minutes until `end` on the next day.
        (MINUTES_IN_DAY - current_minute) + end
    } else {
        // Wrap-around, already past midnight, `end` is later today.
        (end - current_minute).max(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Datelike;

    fn prefs(start: i32, end: i32) -> NotificationPreferences {
        NotificationPreferences::new("Europe/Moscow", start, end).expect("valid")
    }

    #[test]
    fn given_disabled_window_when_in_middle_of_day_then_not_in_quiet_hours() {
        let p = prefs(0, 0);
        let now = Utc::now();
        assert!(!p.is_in_quiet_hours(now));
        assert_eq!(p.next_deliverable_at(now), now);
    }

    #[test]
    fn given_wrap_window_22_to_8_when_23_local_then_inside_and_next_is_08() {
        let p = prefs(22 * 60, 8 * 60);
        // Build a UTC instant that maps to 23:00 Moscow local (MSK = UTC+3).
        let now = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 20, 23, 0, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);

        assert!(p.is_in_quiet_hours(now));

        let next = p.next_deliverable_at(now);
        let next_local = next.with_timezone(&p.timezone());
        assert_eq!(next_local.hour(), 8);
        assert_eq!(next_local.minute(), 0);
        assert_eq!(next_local.day(), 21);
    }

    #[test]
    fn given_wrap_window_22_to_8_when_03_local_then_inside_and_next_is_08_same_day() {
        let p = prefs(22 * 60, 8 * 60);
        let now = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 21, 3, 0, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);

        assert!(p.is_in_quiet_hours(now));

        let next = p.next_deliverable_at(now);
        let next_local = next.with_timezone(&p.timezone());
        assert_eq!(next_local.hour(), 8);
        assert_eq!(next_local.day(), 21);
    }

    #[test]
    fn given_wrap_window_when_daytime_local_then_outside_quiet_hours() {
        let p = prefs(22 * 60, 8 * 60);
        let now = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 21, 13, 0, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);
        assert!(!p.is_in_quiet_hours(now));
    }

    #[test]
    fn given_non_wrap_window_when_inside_then_deliverable_at_end() {
        let p = prefs(12 * 60, 14 * 60);
        let now = chrono_tz::Europe::Moscow
            .with_ymd_and_hms(2026, 4, 21, 12, 30, 0)
            .single()
            .expect("unambiguous")
            .with_timezone(&Utc);

        assert!(p.is_in_quiet_hours(now));
        let next_local = p.next_deliverable_at(now).with_timezone(&p.timezone());
        assert_eq!(next_local.hour(), 14);
    }

    #[test]
    fn given_bad_timezone_when_constructed_then_rejected() {
        let err = NotificationPreferences::new("Mars/Olympus_Mons", 0, 0).unwrap_err();
        assert_eq!(err.code(), "NOTIFICATION_PREFERENCES_UNKNOWN_TZ");
    }

    #[test]
    fn given_out_of_range_minutes_when_constructed_then_rejected() {
        let err = NotificationPreferences::new("Europe/Moscow", 0, 2000).unwrap_err();
        assert_eq!(err.code(), "NOTIFICATION_PREFERENCES_MINUTE_OUT_OF_RANGE");
    }
}
