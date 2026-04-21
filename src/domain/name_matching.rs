use std::cmp::Ordering;

use strsim::normalized_levenshtein;

use crate::domain::employee::{Employee, EmployeeMatch, EmployeeMatchOutcome, MatchStrategy};
use crate::shared::constants::limits::MIN_EMPLOYEE_MATCH_CONFIDENCE;

/// Confidence threshold above which a *single* fuzzy candidate is treated
/// as an auto-resolution without asking the user to confirm.  95 matches
/// the behaviour of the prior `ExactFirstName`/`ExactFullName` branches
/// (those already returned 100), while letting "Иванов Иван " collapse to
/// "Иван Иванов" without going through disambiguation.
pub const HIGH_CONFIDENCE_THRESHOLD: u8 = 95;
/// Confidence threshold above which a single fuzzy top-candidate is shown
/// as a *prefilled suggestion* ("Вы имели в виду — Иван Иванов?") rather
/// than just dumped into an ambiguous clarification list.  75 is high
/// enough that the typing-error class ("Ивнов") still gets suggested, but
/// low enough that unrelated first-name matches do not clutter the UI.
pub const SUGGESTED_CONFIDENCE_THRESHOLD: u8 = 75;
/// Confidence assigned to a prefix match (e.g. "ABD" → "Abdullazi").
/// Sits between `SUGGESTED_CONFIDENCE_THRESHOLD` and `HIGH_CONFIDENCE_THRESHOLD`
/// so prefix matches are *always* shown as a suggestion requiring explicit
/// confirmation — they are never silently auto-assigned.
pub const PREFIX_MATCH_CONFIDENCE: u8 = 78;
/// Minimum query length (in Unicode characters) required before the prefix
/// matcher activates.  A 1-character query like "А" would match too many
/// employees to be useful; 2 characters is the practical floor.
const MIN_PREFIX_QUERY_LEN: usize = 2;

pub fn match_employee_name(query: &str, employees: &[Employee]) -> EmployeeMatchOutcome {
    let normalized_query = normalize_name(query);
    let normalized_username_query = normalize_username(query);
    let query_kind = classify_query(query);

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
            strategy: MatchStrategy::ExactUsername,
        });
    }

    if let Some(employee) = employees
        .iter()
        .find(|employee| normalize_name(&employee.full_name) == normalized_query)
    {
        return EmployeeMatchOutcome::Unique(EmployeeMatch {
            employee: employee.clone(),
            confidence: 100,
            strategy: MatchStrategy::ExactFullName,
        });
    }

    if matches!(query_kind, EmployeeQueryKind::FirstName) {
        let exact_first_name_matches = employees
            .iter()
            .filter(|employee| first_name(employee) == normalized_query)
            .map(|employee| EmployeeMatch {
                employee: employee.clone(),
                confidence: 100,
                strategy: MatchStrategy::ExactFirstName,
            })
            .collect::<Vec<_>>();
        if let Some(outcome) = resolve_exact_matches(exact_first_name_matches) {
            return outcome;
        }

        // Prefix match: fires when the query is a proper prefix (not full match)
        // of one or more employees' first names.  "ABD" → "Abdullazi", for
        // example.  Requires ≥ MIN_PREFIX_QUERY_LEN chars to avoid matching
        // a single letter against half the directory.
        if normalized_query.chars().count() >= MIN_PREFIX_QUERY_LEN {
            let mut prefix_matches: Vec<EmployeeMatch> = employees
                .iter()
                .filter(|employee| {
                    let fn_ = first_name(employee);
                    // `starts_with` but NOT an exact match (that's already handled above)
                    fn_.starts_with(normalized_query.as_str())
                        && fn_.chars().count() > normalized_query.chars().count()
                })
                .map(|employee| EmployeeMatch {
                    employee: employee.clone(),
                    confidence: PREFIX_MATCH_CONFIDENCE,
                    strategy: MatchStrategy::PrefixFirstName,
                })
                .collect();

            if !prefix_matches.is_empty() {
                if prefix_matches.len() == 1 {
                    return EmployeeMatchOutcome::Unique(
                        prefix_matches.into_iter().next().expect("len == 1 above"),
                    );
                }
                // Multiple employees share the same prefix — present as ambiguous
                // so the user must pick explicitly.  Sort deterministically by name.
                prefix_matches.sort_by(compare_matches);
                return EmployeeMatchOutcome::Ambiguous(prefix_matches);
            }
        }

        let suggested_first_name_matches = collect_matches(
            employees,
            &normalized_query,
            MatchStrategy::SuggestedFirstName,
            first_name,
        );
        return resolve_suggestions(suggested_first_name_matches);
    }

    let full_name_suggestions = collect_matches(
        employees,
        &normalized_query,
        MatchStrategy::SuggestedFullName,
        |employee| normalize_name(&employee.full_name),
    );
    resolve_suggestions(full_name_suggestions)
}

/// Deliberate "ranking" interpretation of a pool of matches.  The
/// [`AssigneeResolver`] uses this enum to decide whether to auto-resolve,
/// ask the user to confirm a single high-confidence suggestion, or
/// surface a full clarification screen.
#[derive(Debug, Clone)]
pub enum RankedOutcome {
    /// Exactly one candidate with confidence ≥ [`HIGH_CONFIDENCE_THRESHOLD`].
    Unique(EmployeeMatch),
    /// Top candidate with confidence in
    /// `[SUGGESTED_CONFIDENCE_THRESHOLD, HIGH_CONFIDENCE_THRESHOLD)` — we
    /// suggest it to the user but ask for explicit confirmation.
    Suggested(EmployeeMatch, Vec<EmployeeMatch>),
    /// Two or more credible candidates — the caller must render the list.
    Ambiguous(Vec<EmployeeMatch>),
    NotFound,
}

/// Apply the product ranking rules to an [`EmployeeMatchOutcome`] so
/// callers do not have to repeat the `confidence >= 95` / `>= 75`
/// discipline every time they consume a match.
///
/// * `Unique` matches with confidence ≥ `HIGH_CONFIDENCE_THRESHOLD` are
///   returned as `Unique` here (auto-resolve candidate).
/// * `Unique` matches with confidence below that are **demoted** to
///   `Suggested`, which means the UI must render an "Вы имели в виду …?"
///   card — we never silently route a task at 70% confidence.
/// * `Ambiguous` outcomes filter below `SUGGESTED_CONFIDENCE_THRESHOLD`
///   so the clarification list does not include 50%-ish noise.
pub fn rank_outcome(outcome: EmployeeMatchOutcome) -> RankedOutcome {
    match outcome {
        EmployeeMatchOutcome::Unique(top) => {
            if top.confidence >= HIGH_CONFIDENCE_THRESHOLD {
                RankedOutcome::Unique(top)
            } else if top.confidence >= SUGGESTED_CONFIDENCE_THRESHOLD {
                RankedOutcome::Suggested(top, Vec::new())
            } else {
                // Below the suggestion floor.  Downgrade to "not found"
                // to match the "safe by default — never auto-assign"
                // contract.
                RankedOutcome::NotFound
            }
        }
        EmployeeMatchOutcome::Ambiguous(candidates) => {
            let filtered: Vec<_> = candidates
                .into_iter()
                .filter(|candidate| candidate.confidence >= SUGGESTED_CONFIDENCE_THRESHOLD)
                .collect();
            if filtered.is_empty() {
                RankedOutcome::NotFound
            } else if filtered.len() == 1 {
                let top = filtered.into_iter().next().expect("len == 1 above");
                if top.confidence >= HIGH_CONFIDENCE_THRESHOLD {
                    RankedOutcome::Unique(top)
                } else {
                    RankedOutcome::Suggested(top, Vec::new())
                }
            } else {
                // Top candidate at high confidence but others also close —
                // still ask for confirmation because automated routing
                // to the wrong assignee is the costlier error.
                let top = filtered[0].clone();
                let rest = filtered[1..].to_vec();
                if top.confidence >= HIGH_CONFIDENCE_THRESHOLD
                    && rest[0].confidence < SUGGESTED_CONFIDENCE_THRESHOLD
                {
                    RankedOutcome::Unique(top)
                } else if top.confidence >= SUGGESTED_CONFIDENCE_THRESHOLD {
                    RankedOutcome::Suggested(top, rest)
                } else {
                    RankedOutcome::Ambiguous(filtered)
                }
            }
        }
        EmployeeMatchOutcome::NotFound => RankedOutcome::NotFound,
    }
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

fn resolve_exact_matches(matches: Vec<EmployeeMatch>) -> Option<EmployeeMatchOutcome> {
    let first_match = matches.first()?.clone();
    if matches.len() == 1 {
        return Some(EmployeeMatchOutcome::Unique(first_match));
    }

    Some(EmployeeMatchOutcome::Ambiguous(matches))
}

fn resolve_suggestions(matches: Vec<EmployeeMatch>) -> EmployeeMatchOutcome {
    if matches.is_empty() {
        return EmployeeMatchOutcome::NotFound;
    }

    EmployeeMatchOutcome::Ambiguous(matches)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmployeeQueryKind {
    Username,
    FirstName,
    FullName,
}

fn classify_query(value: &str) -> EmployeeQueryKind {
    if value.trim_start().starts_with('@') {
        return EmployeeQueryKind::Username;
    }

    let word_count = normalize_name(value).split_whitespace().count();
    if word_count <= 1 {
        EmployeeQueryKind::FirstName
    } else {
        EmployeeQueryKind::FullName
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use crate::domain::employee::{Employee, MatchStrategy};

    use super::{
        match_employee_name, rank_outcome, EmployeeMatchOutcome, RankedOutcome,
        HIGH_CONFIDENCE_THRESHOLD, PREFIX_MATCH_CONFIDENCE, SUGGESTED_CONFIDENCE_THRESHOLD,
    };

    fn make_employee(full_name: &str, username: Option<&str>) -> Employee {
        Employee {
            id: Some(1),
            full_name: full_name.to_owned(),
            telegram_username: username.map(str::to_owned),
            email: None,
            phone: None,
            department: None,
            is_active: true,
            synced_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    // ── Exact matches ────────────────────────────────────────────────────────

    #[test]
    fn exact_full_name_match_returns_unique_at_100() {
        let employees = vec![make_employee("Иван Иванов", Some("ivanov"))];

        let outcome = match_employee_name("Иван Иванов", &employees);

        assert!(
            matches!(outcome, EmployeeMatchOutcome::Unique(ref m) if m.confidence == 100),
            "exact full name must resolve to Unique(100)"
        );
    }

    #[test]
    fn exact_username_match_returns_unique_at_100() {
        let employees = vec![make_employee("Иван Иванов", Some("ivanov"))];

        let outcome = match_employee_name("@ivanov", &employees);

        assert!(
            matches!(outcome, EmployeeMatchOutcome::Unique(ref m)
                if m.confidence == 100
                    && matches!(m.strategy, MatchStrategy::ExactUsername)),
            "username match must be Unique with ExactUsername strategy"
        );
    }

    #[test]
    fn exact_first_name_single_match_returns_unique_at_100() {
        let employees = vec![
            make_employee("Иван Иванов", Some("ivanov")),
            make_employee("Пётр Петров", None),
        ];

        let outcome = match_employee_name("Иван", &employees);

        assert!(
            matches!(outcome, EmployeeMatchOutcome::Unique(ref m) if m.confidence == 100),
            "unique exact first-name match must resolve to Unique(100)"
        );
    }

    #[test]
    fn exact_first_name_two_matches_returns_ambiguous() {
        let employees = vec![
            make_employee("Иван Иванов", None),
            make_employee("Иван Сидоров", None),
        ];

        let outcome = match_employee_name("Иван", &employees);

        assert!(
            matches!(outcome, EmployeeMatchOutcome::Ambiguous(ref v) if v.len() == 2),
            "duplicate first name must produce Ambiguous"
        );
    }

    // ── Prefix matching ──────────────────────────────────────────────────────

    #[test]
    fn prefix_abbreviation_unique_returns_unique_at_prefix_confidence() {
        // The user requirement: "ABD" → "Abdullazi Zazizov"
        let employees = vec![
            make_employee("Abdullazi Zazizov", None),
            make_employee("Иван Иванов", None),
        ];

        let outcome = match_employee_name("ABD", &employees);

        match outcome {
            EmployeeMatchOutcome::Unique(ref m) => {
                assert_eq!(
                    m.confidence, PREFIX_MATCH_CONFIDENCE,
                    "prefix match confidence must equal PREFIX_MATCH_CONFIDENCE"
                );
                assert!(
                    matches!(m.strategy, MatchStrategy::PrefixFirstName),
                    "strategy must be PrefixFirstName"
                );
                assert_eq!(m.employee.full_name, "Abdullazi Zazizov");
            }
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn prefix_abbreviation_lowercase_is_normalised() {
        // Normalisation folds to lowercase before prefix check.
        let employees = vec![make_employee("Abdullazi Zazizov", None)];

        let outcome = match_employee_name("abd", &employees);

        assert!(
            matches!(outcome, EmployeeMatchOutcome::Unique(ref m)
                if matches!(m.strategy, MatchStrategy::PrefixFirstName)),
            "lowercase prefix must match after normalisation"
        );
    }

    #[test]
    fn prefix_ambiguous_when_multiple_employees_share_prefix() {
        let employees = vec![
            make_employee("Abdullazi Zazizov", None),
            make_employee("Abdulla Karimov", None),
        ];

        let outcome = match_employee_name("Abd", &employees);

        assert!(
            matches!(outcome, EmployeeMatchOutcome::Ambiguous(ref v) if v.len() == 2),
            "shared prefix must produce Ambiguous with all matching employees"
        );
    }

    #[test]
    fn prefix_single_char_query_does_not_match() {
        // A one-character query is below the MIN_PREFIX_QUERY_LEN floor.
        let employees = vec![make_employee("Abdullazi Zazizov", None)];

        let outcome = match_employee_name("A", &employees);

        // Must not produce a prefix match (will fall through to fuzzy, then NotFound)
        assert!(
            !matches!(outcome, EmployeeMatchOutcome::Unique(ref m)
                if matches!(m.strategy, MatchStrategy::PrefixFirstName)),
            "single-char query must not trigger prefix match"
        );
    }

    #[test]
    fn prefix_exact_match_falls_through_to_exact_first_name() {
        // If the query IS the full first name, exact match fires first.
        let employees = vec![make_employee("Abdullazi Zazizov", None)];

        let outcome = match_employee_name("Abdullazi", &employees);

        assert!(
            matches!(outcome, EmployeeMatchOutcome::Unique(ref m)
                if matches!(m.strategy, MatchStrategy::ExactFirstName)),
            "exact first-name query must use ExactFirstName, not PrefixFirstName"
        );
    }

    // ── rank_outcome thresholds ──────────────────────────────────────────────

    #[test]
    fn rank_outcome_unique_above_high_threshold_stays_unique() {
        let emp = make_employee("Иван Иванов", None);
        let m = crate::domain::employee::EmployeeMatch {
            employee: emp,
            confidence: HIGH_CONFIDENCE_THRESHOLD,
            strategy: MatchStrategy::SuggestedFullName,
        };
        let outcome = EmployeeMatchOutcome::Unique(m);

        assert!(matches!(rank_outcome(outcome), RankedOutcome::Unique(_)));
    }

    #[test]
    fn rank_outcome_prefix_confidence_becomes_suggested_not_unique() {
        // PREFIX_MATCH_CONFIDENCE is between SUGGESTED and HIGH thresholds,
        // so it must always surface as Suggested (never auto-assign).
        let emp = make_employee("Abdullazi Zazizov", None);
        let m = crate::domain::employee::EmployeeMatch {
            employee: emp,
            confidence: PREFIX_MATCH_CONFIDENCE,
            strategy: MatchStrategy::PrefixFirstName,
        };
        let outcome = EmployeeMatchOutcome::Unique(m);

        assert!(
            matches!(rank_outcome(outcome), RankedOutcome::Suggested(_, _)),
            "prefix match confidence ({PREFIX_MATCH_CONFIDENCE}) must rank as Suggested, \
             never auto-assigned"
        );
    }

    #[test]
    fn rank_outcome_below_suggested_threshold_becomes_not_found() {
        let emp = make_employee("Иван Иванов", None);
        let m = crate::domain::employee::EmployeeMatch {
            employee: emp,
            confidence: SUGGESTED_CONFIDENCE_THRESHOLD - 1,
            strategy: MatchStrategy::SuggestedFullName,
        };
        let outcome = EmployeeMatchOutcome::Unique(m);

        assert!(
            matches!(rank_outcome(outcome), RankedOutcome::NotFound),
            "confidence below suggestion floor must downgrade to NotFound"
        );
    }

    // ── Cyrillic ё→е normalisation ───────────────────────────────────────────

    #[test]
    fn yo_normalisation_allows_match_regardless_of_letter_variant() {
        let employees = vec![make_employee("Алёша Пушкин", None)];

        // "Алёша" and "Алеша" must both resolve the same employee
        assert!(matches!(
            match_employee_name("Алёша", &employees),
            EmployeeMatchOutcome::Unique(_)
        ));
        assert!(matches!(
            match_employee_name("Алеша", &employees),
            EmployeeMatchOutcome::Unique(_)
        ));
    }

    // ── No match ─────────────────────────────────────────────────────────────

    #[test]
    fn completely_unrelated_query_returns_not_found() {
        let employees = vec![make_employee("Иван Иванов", Some("ivanov"))];

        let outcome = match_employee_name("Зzzz", &employees);

        // Nothing should match at or above the suggestion floor.
        assert!(matches!(rank_outcome(outcome), RankedOutcome::NotFound));
    }
}
