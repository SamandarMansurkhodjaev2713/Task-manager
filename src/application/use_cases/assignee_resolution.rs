use std::sync::Arc;

use crate::application::dto::task_views::{ClarificationRequest, EmployeeCandidateView};
use crate::application::ports::repositories::{EmployeeRepository, UserRepository};
use crate::domain::employee::{Employee, EmployeeMatchOutcome};
use crate::domain::errors::AppResult;
use crate::domain::name_matching::match_employee_name;
use crate::domain::user::User;

pub struct AssigneeResolver {
    user_repository: Arc<dyn UserRepository>,
    employee_repository: Arc<dyn EmployeeRepository>,
}

pub enum AssigneeResolution {
    Resolved {
        user: Option<User>,
        employee: Option<Employee>,
    },
    ClarificationRequired(ClarificationRequest),
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

    pub async fn resolve(&self, query: &str) -> AppResult<AssigneeResolution> {
        let normalized_query = query.trim();
        if normalized_query.is_empty() {
            return Ok(AssigneeResolution::Resolved {
                user: None,
                employee: None,
            });
        }

        let employees = self.employee_repository.list_active().await?;
        let employee_match = match_employee_name(normalized_query, &employees);
        match employee_match {
            EmployeeMatchOutcome::Ambiguous(candidates) => {
                return Ok(AssigneeResolution::ClarificationRequired(
                    ClarificationRequest {
                        message: "Нашёл несколько похожих исполнителей. Уточните имя или username."
                            .to_owned(),
                        candidates: candidates
                            .iter()
                            .map(EmployeeCandidateView::from_match)
                            .collect(),
                    },
                ));
            }
            EmployeeMatchOutcome::Unique(candidate) => {
                let employee = candidate.employee;
                let user =
                    resolve_user_from_employee(self.user_repository.as_ref(), &employee).await?;
                return Ok(AssigneeResolution::Resolved {
                    user,
                    employee: Some(employee),
                });
            }
            EmployeeMatchOutcome::NotFound => {}
        }

        if !looks_like_username(normalized_query) {
            return Ok(AssigneeResolution::ClarificationRequired(
                ClarificationRequest {
                    message:
                        "Не смог точно определить исполнителя. Напишите @username или полное имя."
                            .to_owned(),
                    candidates: Vec::new(),
                },
            ));
        }

        let normalized_username = normalized_query.trim_start_matches('@');
        let user = self
            .user_repository
            .find_by_username(normalized_username)
            .await?;
        if user.is_some() {
            return Ok(AssigneeResolution::Resolved {
                user,
                employee: None,
            });
        }

        Ok(AssigneeResolution::ClarificationRequired(ClarificationRequest {
            message: "Этот @username пока не найден. Попросите исполнителя написать боту /start или укажите другое имя.".to_owned(),
            candidates: Vec::new(),
        }))
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
