use std::sync::Arc;

use crate::application::ports::repositories::EmployeeRepository;
use crate::application::ports::services::EmployeeDirectoryGateway;
use crate::domain::errors::AppResult;

pub struct SyncEmployeesUseCase {
    employee_repository: Arc<dyn EmployeeRepository>,
    employee_directory_gateway: Arc<dyn EmployeeDirectoryGateway>,
}

impl SyncEmployeesUseCase {
    pub fn new(
        employee_repository: Arc<dyn EmployeeRepository>,
        employee_directory_gateway: Arc<dyn EmployeeDirectoryGateway>,
    ) -> Self {
        Self {
            employee_repository,
            employee_directory_gateway,
        }
    }

    pub async fn execute(&self) -> AppResult<usize> {
        let employees = self.employee_directory_gateway.fetch_employees().await?;
        self.employee_repository.upsert_many(&employees).await
    }
}
