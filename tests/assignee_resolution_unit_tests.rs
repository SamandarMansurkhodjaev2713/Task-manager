/// Unit tests for the pure assignee-resolution logic embedded in `name_matching`.
///
/// These tests verify the invariant that task assignment is NEVER silently made to
/// an ambiguous or low-confidence employee — the user must always confirm explicitly.
mod factories;

use telegram_task_bot::domain::employee::EmployeeMatchOutcome;
use telegram_task_bot::domain::name_matching::match_employee_name;

// ─── Shared employee roster ──────────────────────────────────────────────────

fn make_roster() -> Vec<telegram_task_bot::domain::employee::Employee> {
    vec![
        factories::employee("Иван Петров", Some("ivan_petrov")),
        factories::employee("Иван Сидоров", Some("ivan_sidorov")),
        factories::employee("Мария Иванова", Some("maria_ivanova")),
        factories::employee("Мария Петрова", Some("maria_petrova")),
        factories::employee("Алексей Кузнецов", Some("alex_kuznetsov")),
        factories::employee("Дмитрий Волков", Some("dmitry_volkov")),
    ]
}

// ─── Exact-match invariants ───────────────────────────────────────────────────

#[test]
fn given_exact_full_name_when_unique_employee_then_auto_resolved() {
    let roster = make_roster();
    let outcome = match_employee_name("Алексей Кузнецов", &roster);
    assert!(
        matches!(&outcome, EmployeeMatchOutcome::Unique(m) if m.employee.full_name == "Алексей Кузнецов"),
        "exact full name with one match must resolve uniquely"
    );
}

#[test]
fn given_exact_username_with_at_sign_when_unique_employee_then_auto_resolved() {
    let roster = make_roster();
    let outcome = match_employee_name("@dmitry_volkov", &roster);
    assert!(
        matches!(&outcome, EmployeeMatchOutcome::Unique(m) if m.employee.full_name == "Дмитрий Волков"),
        "@username exact match must resolve uniquely"
    );
}

#[test]
fn given_exact_username_without_at_sign_when_unique_employee_then_auto_resolved() {
    // The parser strips leading '@'; bare usernames must still match.
    let roster = make_roster();
    let outcome = match_employee_name("alex_kuznetsov", &roster);
    assert!(
        matches!(&outcome, EmployeeMatchOutcome::Unique(_)),
        "username without @ must still auto-resolve when unique"
    );
}

// ─── Ambiguity safety invariants ─────────────────────────────────────────────

#[test]
fn given_shared_first_name_when_multiple_employees_then_requires_explicit_choice() {
    let roster = make_roster();
    let outcome = match_employee_name("Иван", &roster);
    assert!(
        matches!(outcome, EmployeeMatchOutcome::Ambiguous(_)),
        "shared first name must never auto-resolve — user must pick"
    );
}

#[test]
fn given_shared_first_name_maria_when_two_employees_then_requires_explicit_choice() {
    let roster = make_roster();
    let outcome = match_employee_name("Мария", &roster);
    assert!(
        matches!(outcome, EmployeeMatchOutcome::Ambiguous(_)),
        "shared first name 'Мария' must return Ambiguous, not Unique"
    );
}

// ─── Typo / misspelling safety invariants ────────────────────────────────────

#[test]
fn given_full_name_with_missing_last_letter_when_match_then_never_auto_assigns() {
    // "Алексей Кузнецо" (missing last letter) must NOT auto-assign to "Алексей Кузнецов"
    let roster = make_roster();
    let outcome = match_employee_name("Алексей Кузнецо", &roster);
    assert!(
        !matches!(outcome, EmployeeMatchOutcome::Unique(_)),
        "misspelled full name must not auto-assign — must require clarification"
    );
}

#[test]
fn given_first_name_with_typo_when_match_then_never_returns_unique() {
    let roster = make_roster();
    let outcome = match_employee_name("Ивон", &roster); // typo for "Иван"
    assert!(
        !matches!(outcome, EmployeeMatchOutcome::Unique(_)),
        "typo in first name must not auto-resolve to a unique employee"
    );
}

// ─── Not-found invariants ─────────────────────────────────────────────────────

#[test]
fn given_completely_unknown_name_when_match_then_not_found_or_ambiguous_never_unique() {
    let roster = make_roster();
    let outcome = match_employee_name("Сергей Огурцов", &roster);
    // Must be either NotFound or Ambiguous (low-confidence suggestions) — never Unique.
    assert!(
        !matches!(outcome, EmployeeMatchOutcome::Unique(_)),
        "unknown name must never auto-resolve to a unique employee"
    );
}

#[test]
fn given_empty_query_when_match_then_not_found() {
    let roster = make_roster();
    let outcome = match_employee_name("", &roster);
    assert!(
        matches!(outcome, EmployeeMatchOutcome::NotFound),
        "empty query must return NotFound"
    );
}

#[test]
fn given_whitespace_only_query_when_match_then_not_found() {
    let roster = make_roster();
    let outcome = match_employee_name("   ", &roster);
    assert!(
        matches!(outcome, EmployeeMatchOutcome::NotFound),
        "whitespace-only query must return NotFound"
    );
}

// ─── Normalisation invariants ─────────────────────────────────────────────────

#[test]
fn given_full_name_with_extra_whitespace_when_match_then_resolves_as_exact() {
    let roster = make_roster();
    let outcome = match_employee_name("  Дмитрий   Волков  ", &roster);
    assert!(
        matches!(&outcome, EmployeeMatchOutcome::Unique(m) if m.employee.full_name == "Дмитрий Волков"),
        "leading/trailing/double whitespace must not prevent exact match"
    );
}

#[test]
fn given_full_name_with_yo_substituted_when_roster_has_yo_then_matches() {
    // "ё" and "е" are interchangeable in Russian names (both forms appear in practice).
    let roster = vec![factories::employee(
        "Алёна Фёдорова",
        Some("alena_fedorova"),
    )];
    let outcome = match_employee_name("Алена Федорова", &roster); // "е" instead of "ё"
    assert!(
        matches!(&outcome, EmployeeMatchOutcome::Unique(m) if m.employee.full_name == "Алёна Фёдорова"),
        "ё↔е substitution must succeed for exact full-name match"
    );
}

#[test]
fn given_username_with_uppercase_when_match_then_still_resolves() {
    let roster = make_roster();
    let outcome = match_employee_name("@Ivan_Petrov", &roster); // mixed case
    assert!(
        matches!(&outcome, EmployeeMatchOutcome::Unique(m) if m.employee.full_name == "Иван Петров"),
        "username matching must be case-insensitive"
    );
}
