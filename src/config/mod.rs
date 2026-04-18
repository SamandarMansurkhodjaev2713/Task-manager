use std::env;
use std::num::NonZeroU32;

use secrecy::SecretString;
use serde::Serialize;
use serde_json::json;
use validator::Validate;

use crate::domain::errors::{AppError, AppResult};

const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";
const DEFAULT_RATE_LIMIT_PER_MINUTE: u32 = 20;
const DEFAULT_OPENAI_TRANSCRIPTION_MODEL: &str = "gpt-4o-mini-transcribe";
const DEFAULT_BIND_ADDRESS: &str = "0.0.0.0:8080";
const DEFAULT_LOG_LEVEL: &str = "info,telegram_task_bot=debug";

#[derive(Debug, Clone, Validate, Serialize)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub telegram: TelegramConfig,
    pub google_sheets: GoogleSheetsConfig,
    pub gemini: GeminiConfig,
    pub openai: OpenAiConfig,
    pub http_server: HttpServerConfig,
    pub observability: ObservabilityConfig,
    pub scheduler: SchedulerConfig,
    pub bot: BotBehaviorConfig,
}

#[derive(Debug, Clone, Validate, Serialize)]
pub struct DatabaseConfig {
    #[validate(length(min = 1))]
    pub database_url: String,
}

#[derive(Debug, Clone, Validate, Serialize)]
pub struct TelegramConfig {
    #[serde(skip_serializing)]
    pub bot_token: SecretString,
}

#[derive(Debug, Clone, Validate, Serialize)]
pub struct GoogleSheetsConfig {
    #[validate(length(min = 1))]
    pub spreadsheet_id: String,
    #[validate(length(min = 1))]
    pub range: String,
    #[serde(skip_serializing)]
    pub api_key: Option<SecretString>,
    #[serde(skip_serializing)]
    pub bearer_token: Option<SecretString>,
}

#[derive(Debug, Clone, Validate, Serialize)]
pub struct GeminiConfig {
    #[serde(skip_serializing)]
    pub api_key: SecretString,
    #[validate(length(min = 1))]
    pub model: String,
}

#[derive(Debug, Clone, Validate, Serialize)]
pub struct OpenAiConfig {
    #[serde(skip_serializing)]
    pub api_key: SecretString,
    #[validate(length(min = 1))]
    pub transcription_model: String,
}

#[derive(Debug, Clone, Validate, Serialize)]
pub struct HttpServerConfig {
    #[validate(length(min = 1))]
    pub bind_address: String,
}

#[derive(Debug, Clone, Validate, Serialize)]
pub struct ObservabilityConfig {
    #[validate(length(min = 1))]
    pub log_level: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SchedulerConfig {
    pub employee_sync_interval_minutes: NonZeroU32,
    pub notification_poll_interval_seconds: NonZeroU32,
    pub reminder_tick_seconds: NonZeroU32,
    pub daily_deadline_reminder_hour_utc: u32,
    pub daily_overdue_scan_hour_utc: u32,
    pub daily_summary_hour_utc: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct BotBehaviorConfig {
    pub rate_limit_per_minute: NonZeroU32,
}

impl AppConfig {
    pub fn from_env() -> AppResult<Self> {
        dotenvy::dotenv().ok();

        let config = Self {
            database: DatabaseConfig {
                database_url: required_env("DATABASE_URL")?,
            },
            telegram: TelegramConfig {
                bot_token: SecretString::new(required_env("TELEGRAM_BOT_TOKEN")?.into()),
            },
            google_sheets: GoogleSheetsConfig {
                spreadsheet_id: required_env("GOOGLE_SHEETS_ID")?,
                range: optional_env("GOOGLE_SHEETS_RANGE")
                    .unwrap_or_else(|| "Employees!A:F".to_owned()),
                api_key: optional_secret("GOOGLE_SHEETS_API_KEY"),
                bearer_token: optional_secret("GOOGLE_SHEETS_BEARER_TOKEN"),
            },
            gemini: GeminiConfig {
                api_key: SecretString::new(required_env("GOOGLE_GEMINI_API_KEY")?.into()),
                model: optional_env("GOOGLE_GEMINI_MODEL")
                    .unwrap_or_else(|| DEFAULT_GEMINI_MODEL.to_owned()),
            },
            openai: OpenAiConfig {
                api_key: SecretString::new(required_env("OPENAI_API_KEY")?.into()),
                transcription_model: optional_env("OPENAI_TRANSCRIPTION_MODEL")
                    .unwrap_or_else(|| DEFAULT_OPENAI_TRANSCRIPTION_MODEL.to_owned()),
            },
            http_server: HttpServerConfig {
                bind_address: optional_env("BIND_ADDRESS")
                    .unwrap_or_else(|| DEFAULT_BIND_ADDRESS.to_owned()),
            },
            observability: ObservabilityConfig {
                log_level: optional_env("RUST_LOG").unwrap_or_else(|| DEFAULT_LOG_LEVEL.to_owned()),
            },
            scheduler: SchedulerConfig {
                employee_sync_interval_minutes: non_zero_u32("EMPLOYEE_SYNC_INTERVAL_MINUTES", 60)?,
                notification_poll_interval_seconds: non_zero_u32(
                    "NOTIFICATION_POLL_INTERVAL_SECONDS",
                    30,
                )?,
                reminder_tick_seconds: non_zero_u32("REMINDER_TICK_SECONDS", 60)?,
                daily_deadline_reminder_hour_utc: optional_u32("DEADLINE_REMINDER_HOUR_UTC", 9)?,
                daily_overdue_scan_hour_utc: optional_u32("OVERDUE_SCAN_HOUR_UTC", 10)?,
                daily_summary_hour_utc: optional_u32("DAILY_SUMMARY_HOUR_UTC", 8)?,
            },
            bot: BotBehaviorConfig {
                rate_limit_per_minute: non_zero_u32(
                    "RATE_LIMIT_PER_MINUTE",
                    DEFAULT_RATE_LIMIT_PER_MINUTE,
                )?,
            },
        };

        config.validate_with_rules()?;
        Ok(config)
    }

    fn validate_with_rules(&self) -> AppResult<()> {
        self.validate().map_err(|error| {
            AppError::schema_validation(
                "CONFIG_INVALID",
                "Configuration is invalid",
                json!({ "errors": error.to_string() }),
            )
        })?;

        let has_api_key = self.google_sheets.api_key.is_some();
        let has_bearer_token = self.google_sheets.bearer_token.is_some();

        if !has_api_key && !has_bearer_token {
            return Err(AppError::schema_validation(
                "GOOGLE_SHEETS_AUTH_MISSING",
                "Either GOOGLE_SHEETS_API_KEY or GOOGLE_SHEETS_BEARER_TOKEN must be configured",
                json!({ "spreadsheet_id": self.google_sheets.spreadsheet_id }),
            ));
        }

        if self.scheduler.daily_deadline_reminder_hour_utc > 23
            || self.scheduler.daily_overdue_scan_hour_utc > 23
            || self.scheduler.daily_summary_hour_utc > 23
        {
            return Err(AppError::schema_validation(
                "SCHEDULER_HOUR_INVALID",
                "Scheduler hour must be in the 0..=23 range",
                json!({
                    "deadline_hour": self.scheduler.daily_deadline_reminder_hour_utc,
                    "overdue_hour": self.scheduler.daily_overdue_scan_hour_utc,
                    "daily_summary_hour": self.scheduler.daily_summary_hour_utc,
                }),
            ));
        }

        Ok(())
    }
}

fn required_env(name: &'static str) -> AppResult<String> {
    optional_env(name).ok_or_else(|| {
        AppError::schema_validation(
            "ENV_MISSING",
            format!("Environment variable {name} is required"),
            json!({ "name": name }),
        )
    })
}

fn optional_env(name: &'static str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn optional_secret(name: &'static str) -> Option<SecretString> {
    optional_env(name).map(Into::into).map(SecretString::new)
}

fn non_zero_u32(name: &'static str, default: u32) -> AppResult<NonZeroU32> {
    let value = optional_u32(name, default)?;
    NonZeroU32::new(value).ok_or_else(|| {
        AppError::schema_validation(
            "ENV_NON_ZERO_REQUIRED",
            format!("Environment variable {name} must be greater than zero"),
            json!({ "name": name, "value": value }),
        )
    })
}

fn optional_u32(name: &'static str, default: u32) -> AppResult<u32> {
    match optional_env(name) {
        Some(value) => value.parse::<u32>().map_err(|_| {
            AppError::schema_validation(
                "ENV_NUMBER_INVALID",
                format!("Environment variable {name} must be a positive integer"),
                json!({ "name": name, "value": value }),
            )
        }),
        None => Ok(default),
    }
}
