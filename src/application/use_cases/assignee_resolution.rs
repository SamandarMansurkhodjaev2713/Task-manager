use std::sync::Arc;

use crate::application::dto::task_views::{ClarificationRequest, EmployeeCandidateView};
use crate::application::ports::repositories::{EmployeeRepository, UserRepository};
use crate::domain::employee::Employee;
use crate::domain::errors::AppResult;
use crate::domain::name_matching::{match_employee_name, rank_outcome, RankedOutcome};
use crate::domain::user::User;

pub struct AssigneeResolver {
    user_repository: Arc<dyn UserRepository>,
    employee_repository: Arc<dyn EmployeeRepository>,
}

pub struct ResolvedAssignee {
    pub user: Option<User>,
    pub employee: Option<Employee>,
}

pub enum AssigneeResolution {
    Resolved(Box<ResolvedAssignee>),
    ClarificationRequired(ClarificationRequest),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssigneeResolutionPurpose {
    TaskCreation,
    Reassignment,
}

impl AssigneeResolver {
    pub fn new(
        user_repository: Arc<dyn UserRepository>,
        employee_repository: Arc<dyn EmployeeRepository>,
    ) -> Self {
        Self {
            user_repository,
            employee_repository,
        }
    }

    pub async fn resolve_for_creation(&self, query: &str) -> AppResult<AssigneeResolution> {
        self.resolve_with_purpose(query, AssigneeResolutionPurpose::TaskCreation)
            .await
    }

    pub async fn resolve_for_reassignment(&self, query: &str) -> AppResult<AssigneeResolution> {
        self.resolve_with_purpose(query, AssigneeResolutionPurpose::Reassignment)
            .await
    }

    pub async fn resolve_employee_id(&self, employee_id: i64) -> AppResult<ResolvedAssignee> {
        let employee = self.employee_repository.find_by_id(employee_id).await?;
        let user = match employee.as_ref() {
            Some(employee) => {
                resolve_user_from_employee(self.user_repository.as_ref(), employee).await?
            }
            None => None,
        };

        Ok(ResolvedAssignee { user, employee })
    }

    async fn resolve_with_purpose(
        &self,
        query: &str,
        purpose: AssigneeResolutionPurpose,
    ) -> AppResult<AssigneeResolution> {
        let normalized_query = query.trim();
        if normalized_query.is_empty() {
            return Ok(AssigneeResolution::Resolved(Box::new(ResolvedAssignee {
                user: None,
                employee: None,
            })));
        }

        let employees = self.employee_repository.list_active().await?;
        let raw_outcome = match_employee_name(normalized_query, &employees);
        let ranked = rank_outcome(raw_outcome);
        match ranked {
            RankedOutcome::Unique(candidate) => {
                let employee = candidate.employee;
                let user =
                    resolve_user_from_employee(self.user_repository.as_ref(), &employee).await?;
                return Ok(AssigneeResolution::Resolved(Box::new(ResolvedAssignee {
                    user,
                    employee: Some(employee),
                })));
            }
            RankedOutcome::Suggested(top, rest) => {
                // Prefilled clarification: top suggestion first, alternatives
                // after.  We deliberately show the user a single pre-selected
                // option so the click cost is one tap for the common case.
                let mut candidates = vec![EmployeeCandidateView::from_match(&top)];
                candidates.extend(rest.iter().map(EmployeeCandidateView::from_match));
                return Ok(AssigneeResolution::ClarificationRequired(
                    clarification_request(
                        normalized_query,
                        purpose,
                        suggested_message(&top.employee.full_name),
                        candidates,
                    ),
                ));
            }
            RankedOutcome::Ambiguous(candidates) => {
                return Ok(AssigneeResolution::ClarificationRequired(
                    clarification_request(
                        normalized_query,
                        purpose,
                        clarification_message(normalized_query),
                        candidates
                            .iter()
                            .map(EmployeeCandidateView::from_match)
                            .collect(),
                    ),
                ));
            }
            RankedOutcome::NotFound => {}
        }

        if looks_like_username(normalized_query) {
            let user = self
                .user_repository
                .find_by_username(normalized_query.trim_start_matches('@'))
                .await?;
            if user.is_some() {
                return Ok(AssigneeResolution::Resolved(Box::new(ResolvedAssignee {
                    user,
                    employee: None,
                })));
            }

            return Ok(AssigneeResolution::ClarificationRequired(
                clarification_request(
                    normalized_query,
                    purpose,
                    "Не вижу этого @username среди зарегистрированных пользователей. Попросите коллегу открыть бота через /start или выберите нужного сотрудника ниже.",
                    Vec::new(),
                ),
            ));
        }

        Ok(AssigneeResolution::ClarificationRequired(
            clarification_request(
                normalized_query,
                purpose,
                no_match_message(normalized_query),
                Vec::new(),
            ),
        ))
    }
}

async fn resolve_user_from_employee(
    user_repository: &dyn UserRepository,
    employee: &Employee,
) -> AppResult<Option<User>> {
    let Some(username) = employee.telegram_username.as_deref() else {
        return Ok(None);
    };
    user_repository
        .find_by_username(username.trim_start_matches('@'))
        .await
}

fn clarification_request(
    normalized_query: &str,
    purpose: AssigneeResolutionPurpose,
    message: impl Into<String>,
    candidates: Vec<EmployeeCandidateView>,
) -> ClarificationRequest {
    ClarificationRequest {
        message: message.into(),
        requested_query: Some(normalized_query.to_owned()),
        allow_unassigned: matches!(purpose, AssigneeResolutionPurpose::TaskCreation),
        candidates,
        task_body_preview: None,
    }
}

fn clarification_message(query: &str) -> &'static str {
    if looks_like_full_name(query) {
        return "Нашёл похожих сотрудников, но не могу безопасно назначить задачу автоматически. Выберите правильного человека явно, чтобы задача не ушла не тому.";
    }

    "С этим именем есть несколько сотрудников. Выберите точного исполнителя явно, чтобы задача не ушла не тому."
}

/// Shown when the ranked resolver produced a single top candidate that is
/// confident enough to suggest but not confident enough to auto-route.
/// Keeping this as an owned `String` so we can interpolate the candidate
/// without lifetime gymnastics at the call site.
fn suggested_message(candidate_name: &str) -> String {
    format!(
        "Похоже, вы имели в виду — {candidate_name}. Подтвердите, если это правильный исполнитель, или выберите другого человека."
    )
}

fn no_match_message(query: &str) -> &'static str {
    if looks_like_full_name(query) {
        return "Не могу безопасно сопоставить это полное имя. Проверьте написание, выберите подходящего сотрудника или создайте задачу без исполнителя.";
    }

    "Не могу безопасно определить исполнителя. Укажите @username или точное полное имя, либо создайте задачу без исполнителя."
}

fn looks_like_username(value: &str) -> bool {
    let normalized = value.trim().trim_start_matches('@');
    let length = normalized.chars().count();
    if !(5..=32).contains(&length) {
        return false;
    }

    normalized
        .chars()
        .all(|symbol| symbol.is_ascii_alphanumeric() || symbol == '_')
}

fn looks_like_full_name(value: &str) -> bool {
    value.split_whitespace().count() >= 2
}
