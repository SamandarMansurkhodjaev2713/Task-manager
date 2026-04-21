use chrono::{Datelike, Duration, NaiveDate, Weekday};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::json;

use crate::domain::errors::{AppError, AppResult};
use crate::domain::message::ParsedTaskRequest;
use crate::shared::constants::limits::MIN_TASK_DESCRIPTION_LENGTH;

static ASSIGNEE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?P<name>[А-ЯЁA-Z][А-ЯЁA-Zа-яёa-z]+(?:\s+[А-ЯЁA-Z][А-ЯЁA-Zа-яёa-z]+)?)\s*,\s*")
        .expect("assignee regex must compile")
});
static ASSIGNEE_USERNAME_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^@(?P<username>[A-Za-z0-9_]{5,32})(?:\s*,\s*|\s+)")
        .expect("assignee username regex must compile")
});
static NO_ASSIGNEE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(без исполнителя|без назначения|no assignee)\b")
        .expect("no-assignee regex must compile")
});
static ABSOLUTE_DEADLINE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?:до|к)\s+(?P<day>\d{1,2})[./](?P<month>\d{1,2})(?:[./](?P<year>\d{4}))?")
        .expect("absolute deadline regex must compile")
});
static RELATIVE_DEADLINE_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(?P<raw>сегодня|завтра|послезавтра|срочно)\b")
        .expect("relative deadline regex must compile")
});
static IN_PERIOD_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\bчерез\s+(?P<value>\d+)\s+(?P<unit>дн(?:ей|я)?|час(?:ов|а)?)\b")
        .expect("period deadline regex must compile")
});
static WEEKDAY_REGEX: Lazy<Regex> = Lazy::new(|| {
    // Accept nominative (именительный, "пятница") and genitive
    // (родительный, "пятницы") forms, because the natural Russian phrase
    // for "by Friday" is «до пятницы» / «к пятнице» — not «до пятница».
    // This keeps the kernel's deterministic fallback aligned with how
    // users actually write deadlines.
    Regex::new(r"(?i)\b(?:до|к)?\s*(?P<raw>понедельник(?:а|у)?|вторник(?:а|у)?|сред[аеыу]|четверг(?:а|у)?|пятниц[аеы]|суббот[аеыу]|воскресень[еяю])\b")
        .expect("weekday deadline regex must compile")
});
static WHITESPACE_REGEX: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+").expect("whitespace regex must compile"));

pub fn parse_task_request(message_text: &str, today: NaiveDate) -> AppResult<ParsedTaskRequest> {
    let normalized_message = normalize_whitespace(message_text);
    let (assignee_name, assignee_fragment) = extract_assignee(&normalized_message);
    let explicit_unassigned = NO_ASSIGNEE_REGEX.is_match(&normalized_message);
    let (deadline, deadline_raw) = extract_deadline(&normalized_message, today)?;
    let description = build_description(
        &normalized_message,
        assignee_fragment.as_deref(),
        deadline_raw.as_deref(),
    );

    validate_description(&description)?;

    Ok(ParsedTaskRequest {
        assignee_name,
        task_description: description,
        deadline,
        deadline_raw,
        explicit_unassigned,
        confidence_score: calculate_confidence(assignee_fragment.is_some(), deadline.is_some()),
    })
}

fn extract_assignee(normalized_message: &str) -> (Option<String>, Option<String>) {
    if let Some(captures) = ASSIGNEE_USERNAME_REGEX.captures(normalized_message) {
        if let Some(username) = captures.name("username") {
            let assignee_username = username.as_str().trim().to_owned();
            return (Some(assignee_username), Some(captures[0].to_owned()));
        }
    }

    ASSIGNEE_REGEX
        .captures(normalized_message)
        .and_then(|captures| {
            captures.name("name").map(|name| {
                let assignee_name = normalize_whitespace(name.as_str());
                (Some(assignee_name), Some(captures[0].to_owned()))
            })
        })
        .unwrap_or((None, None))
}

fn extract_deadline(
    message_text: &str,
    today: NaiveDate,
) -> AppResult<(Option<NaiveDate>, Option<String>)> {
    extract_deadline_from_text(message_text, today)
}

/// Public variant that can be called by the unified deadline kernel
/// (`domain::deadline`) without triggering the description validation
/// inside [`parse_task_request`].  Callers that only want to know whether
/// the user mentioned a due date use this directly.
pub fn extract_deadline_from_text(
    message_text: &str,
    today: NaiveDate,
) -> AppResult<(Option<NaiveDate>, Option<String>)> {
    let normalized = normalize_whitespace(message_text);
    if let Some(parsed) = parse_absolute_deadline(&normalized, today)? {
        return Ok(parsed);
    }

    if let Some(parsed) = parse_relative_deadline(&normalized, today) {
        return Ok(parsed);
    }

    if let Some(parsed) = parse_period_deadline(&normalized, today)? {
        return Ok(parsed);
    }

    Ok(parse_weekday_deadline(&normalized, today))
}

fn parse_absolute_deadline(
    message_text: &str,
    today: NaiveDate,
) -> AppResult<Option<(Option<NaiveDate>, Option<String>)>> {
    let Some(captures) = ABSOLUTE_DEADLINE_REGEX.captures(message_text) else {
        return Ok(None);
    };

    let raw = captures[0].trim().to_owned();
    let day = parse_u32(captures.name("day").map(|value| value.as_str()), "day")?;
    let month = parse_u32(captures.name("month").map(|value| value.as_str()), "month")?;
    let year = captures
        .name("year")
        .map(|value| value.as_str().parse::<i32>())
        .transpose()
        .map_err(|_| invalid_deadline_error("year", &raw))?
        .unwrap_or(today.year());

    let deadline = NaiveDate::from_ymd_opt(year, month, day)
        .ok_or_else(|| invalid_deadline_error("date", &raw))?;
    Ok(Some((Some(deadline), Some(raw))))
}

fn parse_relative_deadline(
    message_text: &str,
    today: NaiveDate,
) -> Option<(Option<NaiveDate>, Option<String>)> {
    let captures = RELATIVE_DEADLINE_REGEX.captures(message_text)?;
    let raw = captures.name("raw")?.as_str().to_lowercase();

    let deadline = match raw.as_str() {
        "сегодня" | "срочно" => today,
        "завтра" => today + Duration::days(1),
        "послезавтра" => today + Duration::days(2),
        _ => return None,
    };

    Some((Some(deadline), Some(raw)))
}

fn parse_period_deadline(
    message_text: &str,
    today: NaiveDate,
) -> AppResult<Option<(Option<NaiveDate>, Option<String>)>> {
    let Some(captures) = IN_PERIOD_REGEX.captures(message_text) else {
        return Ok(None);
    };

    let raw = captures[0].trim().to_owned();
    let value = parse_u32(captures.name("value").map(|value| value.as_str()), "value")?;
    let unit = captures
        .name("unit")
        .map(|value| value.as_str())
        .unwrap_or_default();
    let deadline = if unit.starts_with("дн") {
        today + Duration::days(i64::from(value))
    } else {
        let rounded_days = i64::from(value.saturating_add(23) / 24);
        today + Duration::days(rounded_days.max(1))
    };

    Ok(Some((Some(deadline), Some(raw))))
}

fn parse_weekday_deadline(
    message_text: &str,
    today: NaiveDate,
) -> (Option<NaiveDate>, Option<String>) {
    let Some(captures) = WEEKDAY_REGEX.captures(message_text) else {
        return (None, None);
    };

    let Some(raw_match) = captures.name("raw") else {
        return (None, None);
    };

    let Some(target_weekday) = weekday_from_russian(raw_match.as_str()) else {
        return (None, None);
    };

    let days_until = days_until_weekday(today.weekday(), target_weekday);
    let deadline = today + Duration::days(i64::from(days_until));
    (Some(deadline), Some(raw_match.as_str().to_owned()))
}

fn build_description(
    normalized_message: &str,
    assignee_fragment: Option<&str>,
    deadline_fragment: Option<&str>,
) -> String {
    let without_assignee = assignee_fragment
        .map(|fragment| normalized_message.replacen(fragment, "", 1))
        .unwrap_or_else(|| normalized_message.to_owned());
    let without_deadline = deadline_fragment
        .map(|fragment| without_assignee.replacen(fragment, "", 1))
        .unwrap_or(without_assignee);

    normalize_whitespace(
        without_deadline.trim_matches(|symbol: char| symbol == '.' || symbol == ','),
    )
}

fn validate_description(description: &str) -> AppResult<()> {
    if description.chars().count() < MIN_TASK_DESCRIPTION_LENGTH {
        return Err(AppError::business_rule(
            "TASK_DESCRIPTION_TOO_SHORT",
            "Task description is too short",
            json!({ "min_length": MIN_TASK_DESCRIPTION_LENGTH }),
        ));
    }

    let alpha_count = description
        .chars()
        .filter(|value| value.is_alphabetic())
        .count();
    let token_count = description.split_whitespace().count();

    if alpha_count < MIN_TASK_DESCRIPTION_LENGTH || token_count < 2 {
        return Err(AppError::business_rule(
            "TASK_DESCRIPTION_INVALID",
            "Task description looks incomplete or gibberish",
            json!({ "description": description }),
        ));
    }

    Ok(())
}

fn calculate_confidence(has_assignee: bool, has_deadline: bool) -> u8 {
    let mut score = 60;
    if has_assignee {
        score += 20;
    }
    if has_deadline {
        score += 20;
    }
    score
}

fn normalize_whitespace(value: &str) -> String {
    WHITESPACE_REGEX.replace_all(value.trim(), " ").to_string()
}

fn parse_u32(value: Option<&str>, field: &'static str) -> AppResult<u32> {
    value
        .ok_or_else(|| invalid_deadline_error(field, "missing deadline value"))?
        .parse::<u32>()
        .map_err(|_| invalid_deadline_error(field, "invalid deadline value"))
}

fn invalid_deadline_error(field: &'static str, raw: &str) -> AppError {
    AppError::schema_validation(
        "DEADLINE_INVALID",
        "Deadline cannot be parsed",
        json!({ "field": field, "raw": raw }),
    )
}

fn weekday_from_russian(value: &str) -> Option<Weekday> {
    // Normalise to the nominative stem so that "пятницы"/"пятнице" and
    // "пятница" all map to Friday.  We match on the stem prefix rather
    // than an exact equality because Russian declension affixes are short
    // and the regex ensures the input is a well-formed weekday word.
    let lowered = value.to_lowercase();
    if lowered.starts_with("понедельник") {
        return Some(Weekday::Mon);
    }
    if lowered.starts_with("вторник") {
        return Some(Weekday::Tue);
    }
    if lowered.starts_with("сред") {
        return Some(Weekday::Wed);
    }
    if lowered.starts_with("четверг") {
        return Some(Weekday::Thu);
    }
    if lowered.starts_with("пятниц") {
        return Some(Weekday::Fri);
    }
    if lowered.starts_with("суббот") {
        return Some(Weekday::Sat);
    }
    if lowered.starts_with("воскресень") {
        return Some(Weekday::Sun);
    }
    None
}

fn days_until_weekday(current: Weekday, target: Weekday) -> u32 {
    let current_number = current.num_days_from_monday();
    let target_number = target.num_days_from_monday();
    let distance = (target_number + 7 - current_number) % 7;
    if distance == 0 {
        7
    } else {
        distance
    }
}
