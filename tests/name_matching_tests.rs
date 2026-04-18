mod factories;

use telegram_task_bot::domain::employee::EmployeeMatchOutcome;
use telegram_task_bot::domain::name_matching::match_employee_name;

#[test]
fn given_exact_full_name_when_match_then_returns_unique_match() {
    let employees = vec![
        factories::employee("Иван Петров", Some("ivan_petrov")),
        factories::employee("Мария Сидорова", Some("maria_side")),
    ];

    let outcome = match_employee_name("Иван Петров", &employees);

    match outcome {
        EmployeeMatchOutcome::Unique(found) => {
            assert_eq!(found.employee.full_name, "Иван Петров");
        }
        _ => panic!("expected unique match"),
    }
}

#[test]
fn given_ambiguous_first_name_when_match_then_returns_ambiguous_result() {
    let employees = vec![
        factories::employee("Иван Петров", Some("ivan_petrov")),
        factories::employee("Иван Сидоров", Some("ivan_sidorov")),
    ];

    let outcome = match_employee_name("Иван", &employees);

    assert!(matches!(outcome, EmployeeMatchOutcome::Ambiguous(_)));
}

#[test]
fn given_username_when_match_then_returns_unique_match_by_username() {
    let employees = vec![
        factories::employee("Иван Петров", Some("ivan_petrov")),
        factories::employee("Мария Сидорова", Some("maria_side")),
    ];

    let outcome = match_employee_name("@ivan_petrov", &employees);

    match outcome {
        EmployeeMatchOutcome::Unique(found) => {
            assert_eq!(found.employee.full_name, "Иван Петров");
        }
        _ => panic!("expected unique username match"),
    }
}

#[test]
fn given_full_name_with_typo_when_match_then_does_not_auto_assign_unique_employee() {
    let employees = vec![
        factories::employee("Иван Петров", Some("ivan_petrov")),
        factories::employee("Илья Сидоров", Some("ilya_sidorov")),
    ];

    let outcome = match_employee_name("Иван Петро", &employees);

    assert!(!matches!(outcome, EmployeeMatchOutcome::Unique(_)));
}

#[test]
fn given_single_first_name_with_multiple_people_when_match_then_requires_explicit_choice() {
    let employees = vec![
        factories::employee("Мария Иванова", Some("maria_ivanova")),
        factories::employee("Мария Петрова", Some("maria_petrova")),
        factories::employee("Павел Смирнов", Some("pavel_smirnov")),
    ];

    let outcome = match_employee_name("Мария", &employees);

    assert!(matches!(outcome, EmployeeMatchOutcome::Ambiguous(_)));
}
