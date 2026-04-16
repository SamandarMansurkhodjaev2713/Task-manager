use std::cmp::Ordering;

use strsim::normalized_levenshtein;

use crate::domain::employee::{Employee, EmployeeMatch, EmployeeMatchOutcome, MatchStrategy};
use crate::shared::constants::limits::{
    MIN_EMPLOYEE_MATCH_CONFIDENCE, STRONG_EMPLOYEE_MATCH_CONFIDENCE,
};

pub fn match_employee_name(query: &str, employees: &[Employee]) -> EmployeeMatchOutcome {
    let normalized_query = normalize_name(query);
    let normalized_username_query = normalize_username(query);

    if let Some(employee) = employees.iter().find(|employee| {
        employee
            .telegram_username
            .as_deref()
            .map(normalize_username)
            .as_deref()
            == Some(normalized_username_query.as_str())
    }) {
        return EmployeeMatchOutcome::Unique(EmployeeMatch {
            employee: employee.clone(),
            confidence: 100,
            strategy: MatchStrategy::Exact,
        });
    }

    if let Some(employee) = employees
        .iter()
        .find(|employee| normalize_name(&employee.full_name) == normalized_query)
    {
        return EmployeeMatchOutcome::Unique(EmployeeMatch {
            employee: employee.clone(),
            confidence: 100,
            strategy: MatchStrategy::Exact,
        });
    }

    let first_name_matches = collect_matches(
        employees,
        &normalized_query,
        MatchStrategy::FirstNameFuzzy,
        first_name,
    );
    if let Some(outcome) = resolve_matches(first_name_matches) {
        return outcome;
    }

    let partial_matches = collect_matches(
        employees,
        &normalized_query,
        MatchStrategy::PartialFuzzy,
        |employee| normalize_name(&employee.full_name),
    );
    resolve_matches(partial_matches).unwrap_or(EmployeeMatchOutcome::NotFound)
}

fn collect_matches<F>(
    employees: &[Employee],
    normalized_query: &str,
    strategy: MatchStrategy,
    extractor: F,
) -> Vec<EmployeeMatch>
where
    F: Fn(&Employee) -> String,
{
    let mut matches = employees
        .iter()
        .filter_map(|employee| {
            let candidate = extractor(employee);
            let similarity = normalized_levenshtein(normalized_query, &candidate);
            if similarity < MIN_EMPLOYEE_MATCH_CONFIDENCE {
                return None;
            }

            Some(EmployeeMatch {
                employee: employee.clone(),
                confidence: (similarity * 100.0).round() as u8,
                strategy,
            })
        })
        .collect::<Vec<_>>();

    matches.sort_by(compare_matches);
    matches.truncate(3);
    matches
}

fn resolve_matches(matches: Vec<EmployeeMatch>) -> Option<EmployeeMatchOutcome> {
    let first_match = matches.first()?;

    if matches.len() == 1 {
        return Some(EmployeeMatchOutcome::Unique(first_match.clone()));
    }

    let second_confidence = matches
        .get(1)
        .map(|item| item.confidence)
        .unwrap_or_default();
    let confidence_gap = first_match.confidence.saturating_sub(second_confidence);
    if f64::from(first_match.confidence) / 100.0 >= STRONG_EMPLOYEE_MATCH_CONFIDENCE
        && confidence_gap >= 5
    {
        return Some(EmployeeMatchOutcome::Unique(first_match.clone()));
    }

    Some(EmployeeMatchOutcome::Ambiguous(matches))
}

fn compare_matches(left: &EmployeeMatch, right: &EmployeeMatch) -> Ordering {
    right
        .confidence
        .cmp(&left.confidence)
        .then_with(|| left.employee.full_name.cmp(&right.employee.full_name))
}

fn first_name(employee: &Employee) -> String {
    employee
        .full_name
        .split_whitespace()
        .next()
        .map(normalize_name)
        .unwrap_or_default()
}

fn normalize_name(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .replace('ё', "е")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_username(value: &str) -> String {
    value.trim().trim_start_matches('@').to_lowercase()
}
