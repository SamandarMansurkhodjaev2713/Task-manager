//! Regression tests for the ranked-match interpretation of
//! `EmployeeMatchOutcome`.
//!
//! Senior-engineer notes: the ranking function never invents candidates,
//! never lowers a 100-confidence result, and never silently routes a
//! task at borderline confidence.  When the model is unsure the caller
//! **must** ask for confirmation — these tests fence in that contract.

use chrono::Utc;
use telegram_task_bot::domain::employee::{
    Employee, EmployeeMatch, EmployeeMatchOutcome, MatchStrategy,
};
use telegram_task_bot::domain::name_matching::{
    rank_outcome, RankedOutcome, HIGH_CONFIDENCE_THRESHOLD, SUGGESTED_CONFIDENCE_THRESHOLD,
};

fn employee(name: &str) -> Employee {
    Employee {
        id: Some(1),
        full_name: name.to_owned(),
        telegram_username: None,
        email: None,
        phone: None,
        department: None,
        is_active: true,
        synced_at: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn match_with(name: &str, confidence: u8) -> EmployeeMatch {
    EmployeeMatch {
        employee: employee(name),
        confidence,
        strategy: MatchStrategy::SuggestedFullName,
    }
}

#[test]
fn given_unique_high_confidence_when_ranked_then_auto_resolves() {
    let outcome = EmployeeMatchOutcome::Unique(match_with("Иван Иванов", 100));

    let ranked = rank_outcome(outcome);

    assert!(matches!(ranked, RankedOutcome::Unique(m) if m.employee.full_name == "Иван Иванов"));
}

#[test]
fn given_unique_medium_confidence_when_ranked_then_demoted_to_suggested() {
    let outcome = EmployeeMatchOutcome::Unique(match_with(
        "Иван Иванов",
        SUGGESTED_CONFIDENCE_THRESHOLD + 2,
    ));

    let ranked = rank_outcome(outcome);

    assert!(matches!(
        ranked,
        RankedOutcome::Suggested(top, rest) if top.employee.full_name == "Иван Иванов" && rest.is_empty()
    ));
}

#[test]
fn given_unique_below_suggested_threshold_when_ranked_then_not_found() {
    let outcome = EmployeeMatchOutcome::Unique(match_with(
        "Иван Иванов",
        SUGGESTED_CONFIDENCE_THRESHOLD - 1,
    ));

    assert!(matches!(rank_outcome(outcome), RankedOutcome::NotFound));
}

#[test]
fn given_ambiguous_with_low_noise_when_ranked_then_noise_filtered() {
    let outcome = EmployeeMatchOutcome::Ambiguous(vec![
        match_with("Иван Иванов", HIGH_CONFIDENCE_THRESHOLD),
        match_with("Иван Сидоров", SUGGESTED_CONFIDENCE_THRESHOLD + 2),
        match_with("Иван Какой-то", 50),
    ]);

    let ranked = rank_outcome(outcome);

    match ranked {
        RankedOutcome::Suggested(top, rest) => {
            assert_eq!(top.employee.full_name, "Иван Иванов");
            assert_eq!(rest.len(), 1);
            assert_eq!(rest[0].employee.full_name, "Иван Сидоров");
        }
        other => panic!("expected Suggested, got {other:?}"),
    }
}

#[test]
fn given_ambiguous_collapses_to_single_high_confidence_when_ranked_then_unique() {
    let outcome = EmployeeMatchOutcome::Ambiguous(vec![
        match_with("Иван Иванов", 100),
        match_with("Иван Сидоров", 40), // will be filtered out
    ]);

    let ranked = rank_outcome(outcome);

    match ranked {
        RankedOutcome::Unique(top) => {
            assert_eq!(top.employee.full_name, "Иван Иванов");
        }
        other => panic!("expected Unique, got {other:?}"),
    }
}

#[test]
fn given_ambiguous_all_noise_when_ranked_then_not_found() {
    let outcome = EmployeeMatchOutcome::Ambiguous(vec![match_with("A", 50), match_with("B", 30)]);

    assert!(matches!(rank_outcome(outcome), RankedOutcome::NotFound));
}

#[test]
fn given_not_found_outcome_when_ranked_then_still_not_found() {
    assert!(matches!(
        rank_outcome(EmployeeMatchOutcome::NotFound),
        RankedOutcome::NotFound
    ));
}

#[test]
fn given_two_high_confidence_competitors_when_ranked_then_ambiguous() {
    let outcome = EmployeeMatchOutcome::Ambiguous(vec![
        match_with("Иван Иванов", 97),
        match_with("Иван Иванцов", 96),
    ]);

    let ranked = rank_outcome(outcome);

    // Both candidates clear the HIGH threshold, so the caller must force
    // a confirmation screen rather than auto-route.
    match ranked {
        RankedOutcome::Suggested(top, rest) => {
            assert_eq!(top.confidence, 97);
            assert_eq!(rest.len(), 1);
            assert_eq!(rest[0].confidence, 96);
        }
        other => panic!("expected Suggested, got {other:?}"),
    }
}
