//! Google Sheets write-back adapter.
//!
//! Appends a single row representing a `bot_registered` employee to the
//! operator-configured write-back range using the Sheets API `values.append`
//! endpoint.
//!
//! # Authentication
//!
//! Requires `GOOGLE_SHEETS_BEARER_TOKEN` (OAuth 2.0 access token).  The
//! read-only `GOOGLE_SHEETS_API_KEY` is not sufficient — the Sheets API
//! rejects append requests authenticated with a plain API key.
//!
//! # No-op stub
//!
//! When write-back is not configured (missing bearer token or write-back
//! range), the application wires a [`NoOpSheetsWriteBack`] instance so the
//! rest of the code never has to check for `None`.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::Client;
use secrecy::ExposeSecret;
use serde_json::json;

use crate::application::ports::services::SheetsWriteBackGateway;
use crate::config::GoogleSheetsConfig;
use crate::domain::errors::{AppError, AppResult};
use crate::infrastructure::http::circuit_breaker::CircuitBreaker;
use crate::infrastructure::http::retry::retry_with_backoff;
use crate::shared::constants::timeouts::GOOGLE_SHEETS_TIMEOUT_SECONDS;

const GOOGLE_SHEETS_VALUES_URL: &str = "https://sheets.googleapis.com/v4/spreadsheets";

/// Live adapter that appends rows to a Google Sheet.
pub struct GoogleSheetsWriteBackClient {
    client: Client,
    config: GoogleSheetsConfig,
    /// The A1 notation range in the sheet where rows should be appended,
    /// e.g. `"Employees!A:F"`.
    write_back_range: String,
    circuit_breaker: CircuitBreaker,
}

impl GoogleSheetsWriteBackClient {
    pub fn new(config: GoogleSheetsConfig, write_back_range: String) -> AppResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(GOOGLE_SHEETS_TIMEOUT_SECONDS))
            .build()
            .map_err(|error| {
                AppError::internal(
                    "HTTP_CLIENT_BUILD_FAILED",
                    "Failed to create Google Sheets write-back HTTP client",
                    json!({ "error": error.to_string() }),
                )
            })?;
        Ok(Self {
            client,
            config,
            write_back_range,
            circuit_breaker: CircuitBreaker::new(),
        })
    }
}

#[async_trait]
impl SheetsWriteBackGateway for GoogleSheetsWriteBackClient {
    async fn append_employee_row(
        &self,
        full_name: &str,
        telegram_username: Option<&str>,
        telegram_id: i64,
    ) -> AppResult<()> {
        self.circuit_breaker
            .ensure_closed("sheets_write_back")
            .await?;

        let Some(bearer_token) = &self.config.bearer_token else {
            return Err(AppError::internal(
                "SHEETS_WRITE_BACK_NO_AUTH",
                "Bearer token is required for Google Sheets write-back",
                json!({}),
            ));
        };

        let url = format!(
            "{}/{}/values/{}:append?valueInputOption=USER_ENTERED&insertDataOption=INSERT_ROWS",
            GOOGLE_SHEETS_VALUES_URL,
            self.config.spreadsheet_id,
            urlencoding::encode(&self.write_back_range),
        );

        let username_cell = telegram_username
            .map(|u| format!("@{u}"))
            .unwrap_or_default();
        let telegram_id_str = telegram_id.to_string();

        let body = json!({
            "values": [[full_name, username_cell, "", "", "", "true", telegram_id_str]]
        });

        let do_request = || {
            let client = self.client.clone();
            let url = url.clone();
            let body = body.clone();
            let token = bearer_token.expose_secret().to_owned();
            async move {
                client
                    .post(&url)
                    .bearer_auth(&token)
                    .json(&body)
                    .send()
                    .await
                    .map_err(|error| {
                        AppError::network(
                            "SHEETS_WRITE_BACK_REQUEST_FAILED",
                            "Failed to append row to Google Sheets",
                            json!({ "error": error.to_string() }),
                        )
                    })
            }
        };

        let response = retry_with_backoff(do_request).await?;

        if response.status().is_success() {
            self.circuit_breaker.record_success().await;
            Ok(())
        } else {
            self.circuit_breaker.record_failure().await;
            Err(AppError::network(
                "SHEETS_WRITE_BACK_HTTP_STATUS",
                "Google Sheets append returned a non-success status",
                json!({ "status": response.status().as_u16() }),
            ))
        }
    }
}

/// No-op stub used when write-back is disabled.
pub struct NoOpSheetsWriteBack;

#[async_trait]
impl SheetsWriteBackGateway for NoOpSheetsWriteBack {
    async fn append_employee_row(
        &self,
        _full_name: &str,
        _telegram_username: Option<&str>,
        _telegram_id: i64,
    ) -> AppResult<()> {
        Ok(())
    }
}
