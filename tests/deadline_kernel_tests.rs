//! Integration-level regression tests for the deadline kernel.
//!
//! Mirrors the unit tests inside `src/domain/deadline.rs` so we can execute
//! them as a standalone binary when AppLocker blocks the lib-test binary on
//! this developer machine.  The behaviours fenced in here are the ones we
//! advertise to the rest of the codebase (AI wins, past AI rejected,
//! weekend rolls forward, fallback works).

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Timelike, Utc};
use chrono_tz::Europe::Moscow;
use chrono_tz::Tz;
use telegram_task_bot::domain::deadline::{
    Deadline, DeadlineInput, DeadlineResolver, DeadlineSource,
};
use telegram_task_bot::domain::sla::WorkingCalendar;

fn moscow_at(y: i32, m: u32, d: u32, h: u32) -> DateTime<Utc> {
    Moscow
        .with_ymd_and_hms(y, m, d, h, 0, 0)
        .single()
        .expect("unambiguous")
        .with_timezone(&Utc)
}

fn business_calendar() -> WorkingCalendar {
    WorkingCalendar::new(
        "RU_STANDARD",
        "Europe/Moscow".parse::<Tz>().expect("valid tz"),
        (1 << 0) | (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4),
        540,
        1080,
        Vec::<NaiveDate>::new(),
        Vec::<NaiveDate>::new(),
    )
    .expect("valid calendar")
}

fn assert_local_eq(deadline: &Deadline, day: u32, hour: u32) {
    let utc = deadline.utc.expect("has value");
    let local = utc.with_timezone(&Moscow);
    assert_eq!(local.day(), day, "expected day={day}, got {local}");
    assert_eq!(local.hour(), hour, "expected hour={hour}, got {local}");
}

#[test]
fn given_ai_structured_hint_when_resolving_then_ai_source_wins() {
    let now = moscow_at(2026, 4, 20, 9); // Monday morning

    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "сделать отчёт",
        ai_iso_hint: Some("2026-04-24T15:00:00+03:00"),
        user_timezone: Moscow,
        now_utc: now,
        calendar: None,
    })
    .expect("resolves");

    assert_eq!(resolved.source, DeadlineSource::AiStructured);
    assert_eq!(resolved.confidence, 100);
    assert_local_eq(&resolved, 24, 15);
}

#[test]
fn given_past_ai_hint_when_resolving_then_fallback_to_text_wins() {
    let now = moscow_at(2026, 4, 20, 9);

    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "отчёт до завтра",
        ai_iso_hint: Some("2000-01-01T00:00:00Z"),
        user_timezone: Moscow,
        now_utc: now,
        calendar: None,
    })
    .expect("resolves");

    assert_eq!(resolved.source, DeadlineSource::DeterministicText);
    assert_local_eq(&resolved, 21, 18);
}

#[test]
fn given_weekend_ai_hint_with_calendar_when_resolving_then_rolls_to_monday() {
    let now = moscow_at(2026, 4, 20, 9); // Monday
    let calendar = business_calendar();

    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "",
        ai_iso_hint: Some("2026-04-25"), // Saturday
        user_timezone: Moscow,
        now_utc: now,
        calendar: Some(&calendar),
    })
    .expect("resolves");

    let utc = resolved.utc.unwrap();
    let local = utc.with_timezone(&Moscow);
    assert_eq!(
        local.weekday(),
        chrono::Weekday::Mon,
        "expected Monday, got {local}"
    );
    assert_eq!(local.hour(), 18);
}

#[test]
fn given_no_deadline_anywhere_when_resolving_then_returns_none() {
    let now = moscow_at(2026, 4, 20, 9);

    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "подготовить релизный чек-лист",
        ai_iso_hint: None,
        user_timezone: Moscow,
        now_utc: now,
        calendar: None,
    })
    .expect("resolves");

    assert!(!resolved.has_value());
    assert_eq!(resolved.source, DeadlineSource::NoneProvided);
    assert_eq!(resolved.confidence, 0);
}

#[test]
fn given_ai_hint_date_only_when_resolving_then_clamps_to_end_of_business() {
    let now = moscow_at(2026, 4, 20, 6);

    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "",
        ai_iso_hint: Some("2026-04-22"),
        user_timezone: Moscow,
        now_utc: now,
        calendar: None,
    })
    .expect("resolves");

    assert_eq!(resolved.source, DeadlineSource::AiStructured);
    assert_local_eq(&resolved, 22, 18);
}

#[test]
fn given_weekday_phrase_in_text_when_resolving_then_confidence_bracketed() {
    let now = moscow_at(2026, 4, 20, 12); // Monday
    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "до пятницы",
        ai_iso_hint: None,
        user_timezone: Moscow,
        now_utc: now,
        calendar: None,
    })
    .expect("resolves");

    assert!(resolved.has_value());
    assert!(
        resolved.confidence >= 60,
        "weekday confidence: {}",
        resolved.confidence
    );
    assert_local_eq(&resolved, 24, 18); // Friday
}

#[test]
fn given_relative_phrase_three_days_when_resolving_then_uses_user_timezone_today() {
    let now = moscow_at(2026, 4, 20, 22); // Monday 22:00 Moscow (but past midnight UTC+3)
    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "через 3 дня",
        ai_iso_hint: None,
        user_timezone: Moscow,
        now_utc: now,
        calendar: None,
    })
    .expect("resolves");

    assert_eq!(resolved.source, DeadlineSource::DeterministicText);
    let local = resolved.utc.unwrap().with_timezone(&Moscow);
    let three_days = now.with_timezone(&Moscow).date_naive() + Duration::days(3);
    assert_eq!(local.date_naive(), three_days);
}

#[test]
fn given_local_label_on_no_deadline_when_rendering_then_returns_none_option() {
    let now = moscow_at(2026, 4, 20, 9);
    let resolved = DeadlineResolver::resolve(DeadlineInput {
        text: "просто текст",
        ai_iso_hint: None,
        user_timezone: Moscow,
        now_utc: now,
        calendar: None,
    })
    .expect("resolves");

    assert!(resolved.local_label().is_none());
}
