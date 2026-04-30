use async_trait::async_trait;
use chrono::Utc;
use serde::Deserialize;
use tokio::fs;

use crate::application::ports::services::EmployeeDirectoryGateway;
use crate::domain::employee::{Employee, EmployeeSource};
use crate::domain::errors::{AppError, AppResult};

pub struct LocalEmployeeDirectory {
    csv_path: Option<String>,
}

impl LocalEmployeeDirectory {
    pub fn empty() -> Self {
        Self { csv_path: None }
    }

    pub fn from_csv(csv_path: String) -> Self {
        Self {
            csv_path: Some(csv_path),
        }
    }
}

#[async_trait]
impl EmployeeDirectoryGateway for LocalEmployeeDirectory {
    async fn fetch_employees(&self) -> AppResult<Vec<Employee>> {
        let Some(csv_path) = &self.csv_path else {
            return Ok(Vec::new());
        };

        let csv = fs::read_to_string(csv_path).await.map_err(|error| {
            AppError::network(
                "LOCAL_EMPLOYEE_DIRECTORY_READ_FAILED",
                "Failed to read local employee directory CSV",
                serde_json::json!({ "path": csv_path, "error": error.to_string() }),
            )
        })?;

        let mut reader = csv::ReaderBuilder::new()
            .trim(csv::Trim::All)
            .from_reader(csv.as_bytes());
        let now = Utc::now();
        let mut employees = Vec::new();

        for row in reader.deserialize::<LocalEmployeeRow>() {
            let row = row.map_err(|error| {
                AppError::schema_validation(
                    "LOCAL_EMPLOYEE_DIRECTORY_ROW_INVALID",
                    "Local employee directory CSV row is invalid",
                    serde_json::json!({ "path": csv_path, "error": error.to_string() }),
                )
            })?;

            let full_name = row.full_name.trim().to_owned();
            if full_name.is_empty() {
                continue;
            }

            employees.push(Employee {
                id: None,
                full_name,
                telegram_username: clean_optional(row.telegram_username)
                    .map(|value| value.trim_start_matches('@').to_owned()),
                email: clean_optional(row.email),
                phone: clean_optional(row.phone),
                department: clean_optional(row.department),
                is_active: !row.is_active.trim().eq_ignore_ascii_case("false"),
                source: EmployeeSource::GoogleSheets,
                synced_at: Some(now),
                created_at: now,
                updated_at: now,
            });
        }

        Ok(employees)
    }
}

fn clean_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

#[derive(Debug, Deserialize)]
struct LocalEmployeeRow {
    full_name: String,
    #[serde(default)]
    telegram_username: String,
    #[serde(default)]
    email: String,
    #[serde(default)]
    phone: String,
    #[serde(default)]
    department: String,
    #[serde(default = "default_true")]
    is_active: String,
}

fn default_true() -> String {
    "true".to_owned()
}

#[cfg(test)]
mod tests {
    use tempfile::NamedTempFile;
    use tokio::io::AsyncWriteExt;

    use super::*;

    #[tokio::test]
    async fn given_local_employee_csv_when_fetching_then_parses_employees() {
        let file = NamedTempFile::new().expect("temp file");
        let path = file.path().to_owned();
        let mut async_file = fs::File::create(&path).await.expect("create csv");
        async_file
            .write_all(
                "full_name,telegram_username,email,phone,department,is_active\n\
                 Иван Иванов,@ivan.ivanov,ivan@example.com,+998,Сервис,true\n\
                 Неактивный Пользователь,inactive,,,,false\n"
                    .as_bytes(),
            )
            .await
            .expect("write csv");
        drop(async_file);

        let directory = LocalEmployeeDirectory::from_csv(path.to_string_lossy().to_string());
        let employees = directory.fetch_employees().await.expect("employees");

        assert_eq!(employees.len(), 2);
        assert_eq!(employees[0].full_name, "Иван Иванов");
        assert_eq!(
            employees[0].telegram_username.as_deref(),
            Some("ivan.ivanov")
        );
        assert!(employees[0].is_active);
        assert!(!employees[1].is_active);
    }
}
