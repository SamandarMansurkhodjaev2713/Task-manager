use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Client;
use secrecy::ExposeSecret;
use serde::Deserialize;

use crate::application::ports::services::EmployeeDirectoryGateway;
use crate::config::GoogleSheetsConfig;
use crate::domain::employee::Employee;
use crate::domain::errors::{AppError, AppResult};
use crate::infrastructure::http::circuit_breaker::CircuitBreaker;
use crate::infrastructure::http::retry::retry_with_backoff;
use crate::shared::constants::timeouts::GOOGLE_SHEETS_TIMEOUT_SECONDS;

const GOOGLE_SHEETS_VALUES_URL: &str = "https://sheets.googleapis.com/v4/spreadsheets";

pub struct GoogleSheetsEmployeeDirectory {
    client: Client,
    config: GoogleSheetsConfig,
    circuit_breaker: CircuitBreaker,
}

impl GoogleSheetsEmployeeDirectory {
    pub fn new(config: GoogleSheetsConfig) -> AppResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(GOOGLE_SHEETS_TIMEOUT_SECONDS))
            .build()
            .map_err(|error| {
                AppError::internal(
                    "HTTP_CLIENT_BUILD_FAILED",
                    "Failed to create Google Sheets HTTP client",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;

        Ok(Self {
            client,
            config,
            circuit_breaker: CircuitBreaker::new(),
        })
    }

    async fn fetch_rows(&self) -> AppResult<Vec<Vec<String>>> {
        self.circuit_breaker.ensure_closed("google_sheets").await?;

        let url = format!(
            "{}/{}/values/{}",
            GOOGLE_SHEETS_VALUES_URL,
            self.config.spreadsheet_id,
            urlencoding::encode(&self.config.range)
        );
        let mut request = self.client.get(url);

        if let Some(bearer_token) = &self.config.bearer_token {
            request = request.bearer_auth(bearer_token.expose_secret());
        } else if let Some(api_key) = &self.config.api_key {
            request = request.query(&[("key", api_key.expose_secret())]);
        }

        let response = request.send().await.map_err(|error| {
            AppError::network(
                "GOOGLE_SHEETS_REQUEST_FAILED",
                "Failed to request employees from Google Sheets",
                serde_json::json!({ "error": error.to_string() }),
            )
        })?;

        if !response.status().is_success() {
            self.circuit_breaker.record_failure().await;
            return Err(AppError::network(
                "GOOGLE_SHEETS_HTTP_STATUS",
                "Google Sheets returned a non-success status code",
                serde_json::json!({ "status": response.status().as_u16() }),
            ));
        }

        let payload = response
            .json::<GoogleSheetsValuesResponse>()
            .await
            .map_err(|error| {
                AppError::network(
                    "GOOGLE_SHEETS_RESPONSE_INVALID",
                    "Google Sheets response is invalid",
                    serde_json::json!({ "error": error.to_string() }),
                )
            })?;
        self.circuit_breaker.record_success().await;
        Ok(payload.values.unwrap_or_default())
    }
}

#[async_trait]
impl EmployeeDirectoryGateway for GoogleSheetsEmployeeDirectory {
    async fn fetch_employees(&self) -> AppResult<Vec<Employee>> {
        let rows = retry_with_backoff(|| self.fetch_rows()).await?;
        let now = Utc::now();

        Ok(rows
            .into_iter()
            .enumerate()
            .filter_map(|row| {
                let (index, row) = row;
                if index == 0 && is_header_row(&row) {
                    return None;
                }
                let full_name = row.first()?.trim().to_owned();
                if full_name.is_empty() {
                    return None;
                }

                Some(Employee {
                    id: None,
                    full_name,
                    telegram_username: row
                        .get(1)
                        .map(|value| value.trim().trim_start_matches('@').to_owned())
                        .filter(|value| !value.is_empty()),
                    email: row
                        .get(2)
                        .map(|value| value.trim().to_owned())
                        .filter(|value| !value.is_empty()),
                    phone: row
                        .get(3)
                        .map(|value| value.trim().to_owned())
                        .filter(|value| !value.is_empty()),
                    department: row
                        .get(4)
                        .map(|value| value.trim().to_owned())
                        .filter(|value| !value.is_empty()),
                    is_active: row
                        .get(5)
                        .map(|value| !value.trim().eq_ignore_ascii_case("false"))
                        .unwrap_or(true),
                    synced_at: Some(now),
                    created_at: now,
                    updated_at: now,
                })
            })
            .collect())
    }
}

fn is_header_row(row: &[String]) -> bool {
    let normalized_cells = row
        .iter()
        .map(|value| value.trim().to_lowercase().replace(' ', "_"))
        .collect::<Vec<_>>();

    normalized_cells.iter().any(|value| {
        matches!(
            value.as_str(),
            "full_name" | "telegram_username" | "email" | "phone" | "department" | "is_active"
        )
    })
}

#[derive(Debug, Deserialize)]
struct GoogleSheetsValuesResponse {
    values: Option<Vec<Vec<String>>>,
}
