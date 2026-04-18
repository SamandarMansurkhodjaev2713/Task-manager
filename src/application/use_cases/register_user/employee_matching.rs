//! Registration-time employee matching.
//!
//! Determines which (if any) employee-directory record corresponds to the
//! Telegram user who just sent `/start`.  Unlike the general-purpose
//! [`match_employee_name`](crate::domain::name_matching::match_employee_name)
//! fuzzy matcher, registration matching is intentionally strict:
//!
//! - Username matching takes priority and is case-insensitive.
//! - Full-name matching falls back when no username match exists.
//! - Fuzzy / partial matches are deliberately excluded — a registration
//!   must be unambiguous or the user is presented with an explicit choice.
//!
//! The `RegistrationEmployeeMatch` enum mirrors `EmployeeMatchOutcome` in spirit
//! but is separate because registration semantics differ slightly (the
//! `Ambiguous` arms carry `EmployeeMatch` slices rather than raw `Employee`
//! values, and `NotFound` is a valid and expected outcome here).

use crate::application::dto::task_views::EmployeeCandidateView;
use crate::domain::employee::{Employee, EmployeeMatch, MatchStrategy};
use crate::domain::message::IncomingMessage;

/// Outcome of matching a registering user against the employee directory.
pub(super) enum RegistrationEmployeeMatch {
    /// Exactly one matching employee was found — auto-link is safe.
    Unique(Employee),
    /// Multiple employees matched — the user must pick explicitly.
    Ambiguous(Vec<EmployeeMatch>),
    /// No match found — user registers without an employee link.
    NotFound,
}

/// Attempts to match `message.sender_username` / `sender_name` against
/// the active employee roster and returns the strictest matching outcome.
///
/// Username matches (exact, case-insensitive) are tried first; full-name
/// matches (exact, normalised) are the fallback.
pub(super) fn resolve_registration_match(
    message: &IncomingMessage,
    employees: &[Employee],
) -> RegistrationEmployeeMatch {
    let username_matches = exact_username_matches(message, employees);
    if username_matches.len() == 1 {
        return RegistrationEmployeeMatch::Unique(username_matches.into_iter().next().unwrap());
    }
    if !username_matches.is_empty() {
        return RegistrationEmployeeMatch::Ambiguous(
            username_matches
                .into_iter()
                .map(to_employee_match)
                .collect(),
        );
    }

    let full_name_matches = exact_full_name_matches(message, employees);
    if full_name_matches.len() == 1 {
        return RegistrationEmployeeMatch::Unique(full_name_matches.into_iter().next().unwrap());
    }
    if !full_name_matches.is_empty() {
        return RegistrationEmployeeMatch::Ambiguous(
            full_name_matches
                .into_iter()
                .map(to_employee_match)
                .collect(),
        );
    }

    RegistrationEmployeeMatch::NotFound
}

/// Builds a display-friendly `EmployeeCandidateView` from each ambiguous match.
pub(super) fn candidates_from_matches(matches: &[EmployeeMatch]) -> Vec<EmployeeCandidateView> {
    matches
        .iter()
        .map(EmployeeCandidateView::from_match)
        .collect()
}

// ─── Private helpers ─────────────────────────────────────────────────────────

fn exact_username_matches(message: &IncomingMessage, employees: &[Employee]) -> Vec<Employee> {
    let Some(username) = message.sender_username.as_deref() else {
        return Vec::new();
    };
    let normalized_username = username.trim().trim_start_matches('@');
    if normalized_username.is_empty() {
        return Vec::new();
    }

    employees
        .iter()
        .filter(|employee| {
            employee.telegram_username.as_deref().is_some_and(|value| {
                value
                    .trim()
                    .trim_start_matches('@')
                    .eq_ignore_ascii_case(normalized_username)
            })
        })
        .cloned()
        .collect()
}

fn exact_full_name_matches(message: &IncomingMessage, employees: &[Employee]) -> Vec<Employee> {
    let normalized_name = normalize_person_name(&message.sender_name);
    if normalized_name.is_empty() {
        return Vec::new();
    }

    employees
        .iter()
        .filter(|employee| normalize_person_name(&employee.full_name) == normalized_name)
        .cloned()
        .collect()
}

fn to_employee_match(employee: Employee) -> EmployeeMatch {
    EmployeeMatch {
        employee,
        confidence: 100,
        strategy: MatchStrategy::ExactFullName,
    }
}

/// Collapses internal whitespace and lower-cases a person name for comparison.
pub(super) fn normalize_person_name(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{normalize_person_name, resolve_registration_match, RegistrationEmployeeMatch};
    use crate::domain::employee::Employee;
    use crate::domain::message::{IncomingMessage, MessageContent};

    #[test]
    fn given_unique_full_name_when_resolving_registration_match_then_matches_without_username() {
        let employee = make_employee("Jean Dupont", None);
        let message = make_message("Jean Dupont", None);

        let resolved = resolve_registration_match(&message, std::slice::from_ref(&employee));

        assert!(
            matches!(resolved, RegistrationEmployeeMatch::Unique(found) if found.full_name == employee.full_name)
        );
    }

    #[test]
    fn given_duplicate_full_name_when_resolving_registration_match_then_returns_ambiguous() {
        let first = make_employee("Jean Dupont", None);
        let second = make_employee("Jean Dupont", Some("@other"));
        let message = make_message("Jean Dupont", None);

        let resolved = resolve_registration_match(&message, &[first, second]);

        assert!(matches!(resolved, RegistrationEmployeeMatch::Ambiguous(_)));
    }

    #[test]
    fn given_username_when_resolving_registration_match_then_prefers_exact_username_match() {
        let employee = make_employee("Jean Dupont", Some("@jean"));
        let message = make_message("Somebody Else", Some("@jean"));

        let resolved = resolve_registration_match(&message, std::slice::from_ref(&employee));

        assert!(
            matches!(resolved, RegistrationEmployeeMatch::Unique(found) if found.full_name == employee.full_name)
        );
    }

    #[test]
    fn given_name_with_extra_spaces_when_normalizing_then_preserves_single_spaces() {
        assert_eq!(normalize_person_name("  Jean   Dupont  "), "jean dupont");
    }

    fn make_employee(full_name: &str, username: Option<&str>) -> Employee {
        Employee {
            id: Some(1),
            full_name: full_name.to_owned(),
            telegram_username: username.map(|value| value.trim_start_matches('@').to_owned()),
            email: None,
            phone: None,
            department: None,
            is_active: true,
            synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn make_message(sender_name: &str, username: Option<&str>) -> IncomingMessage {
        IncomingMessage {
            chat_id: 1,
            message_id: 1,
            sender_id: 42,
            sender_name: sender_name.to_owned(),
            sender_username: username.map(|value| value.trim_start_matches('@').to_owned()),
            content: MessageContent::Command {
                text: "/start".to_owned(),
            },
            timestamp: Utc::now(),
            source_message_key_override: None,
            is_voice_origin: false,
        }
    }
}
