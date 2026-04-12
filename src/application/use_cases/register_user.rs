use std::sync::Arc;

use crate::application::ports::repositories::{EmployeeRepository, UserRepository};
use crate::domain::errors::AppResult;
use crate::domain::message::IncomingMessage;
use crate::domain::user::{User, UserRole};

pub struct RegisterUserUseCase {
    user_repository: Arc<dyn UserRepository>,
    employee_repository: Arc<dyn EmployeeRepository>,
}

impl RegisterUserUseCase {
    pub fn new(
        user_repository: Arc<dyn UserRepository>,
        employee_repository: Arc<dyn EmployeeRepository>,
    ) -> Self {
        Self {
            user_repository,
            employee_repository,
        }
    }

    pub async fn execute(&self, message: &IncomingMessage) -> AppResult<User> {
        let employees = self.employee_repository.list_active().await?;
        let is_employee = message.sender_username.as_ref().map_or(false, |username| {
            let normalized_username = username.trim_start_matches('@');
            employees.iter().any(|employee| {
                employee
                    .telegram_username
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case(normalized_username))
                    .unwrap_or(false)
            })
        });

        let user = User::from_message(message, UserRole::User, is_employee);
        self.user_repository.upsert_from_message(&user).await
    }
}
