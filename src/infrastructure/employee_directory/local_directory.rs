use async_trait::async_trait;

use crate::application::ports::services::EmployeeDirectoryGateway;
use crate::domain::employee::Employee;
use crate::domain::errors::AppResult;

pub struct LocalEmployeeDirectory;

#[async_trait]
impl EmployeeDirectoryGateway for LocalEmployeeDirectory {
    async fn fetch_employees(&self) -> AppResult<Vec<Employee>> {
        Ok(Vec::new())
    }
}
