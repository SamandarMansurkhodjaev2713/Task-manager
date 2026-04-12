use serde::Serialize;
use serde_json::{json, Value};
use thiserror::Error;

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, Clone, Error, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppError {
    #[error("{message}")]
    Validation {
        code: &'static str,
        message: String,
        context: Value,
    },
    #[error("{message}")]
    Auth {
        code: &'static str,
        message: String,
        context: Value,
    },
    #[error("{message}")]
    NotFound {
        code: &'static str,
        message: String,
        context: Value,
    },
    #[error("{message}")]
    Conflict {
        code: &'static str,
        message: String,
        context: Value,
    },
    #[error("{message}")]
    RateLimit {
        code: &'static str,
        message: String,
        context: Value,
    },
    #[error("{message}")]
    Network {
        code: &'static str,
        message: String,
        context: Value,
    },
    #[error("{message}")]
    Timeout {
        code: &'static str,
        message: String,
        context: Value,
    },
    #[error("{message}")]
    Internal {
        code: &'static str,
        message: String,
        context: Value,
    },
}

impl AppError {
    pub fn schema_validation(
        code: &'static str,
        message: impl Into<String>,
        context: Value,
    ) -> Self {
        Self::Validation {
            code,
            message: message.into(),
            context,
        }
    }

    pub fn business_rule(code: &'static str, message: impl Into<String>, context: Value) -> Self {
        Self::Validation {
            code,
            message: message.into(),
            context,
        }
    }

    pub fn unauthenticated(message: impl Into<String>, context: Value) -> Self {
        Self::Auth {
            code: "UNAUTHENTICATED",
            message: message.into(),
            context,
        }
    }

    pub fn unauthorized(message: impl Into<String>, context: Value) -> Self {
        Self::Auth {
            code: "UNAUTHORIZED",
            message: message.into(),
            context,
        }
    }

    pub fn not_found(code: &'static str, message: impl Into<String>, context: Value) -> Self {
        Self::NotFound {
            code,
            message: message.into(),
            context,
        }
    }

    pub fn conflict(code: &'static str, message: impl Into<String>, context: Value) -> Self {
        Self::Conflict {
            code,
            message: message.into(),
            context,
        }
    }

    pub fn rate_limit(message: impl Into<String>, context: Value) -> Self {
        Self::RateLimit {
            code: "RATE_LIMITED",
            message: message.into(),
            context,
        }
    }

    pub fn network(code: &'static str, message: impl Into<String>, context: Value) -> Self {
        Self::Network {
            code,
            message: message.into(),
            context,
        }
    }

    pub fn timeout(code: &'static str, message: impl Into<String>, context: Value) -> Self {
        Self::Timeout {
            code,
            message: message.into(),
            context,
        }
    }

    pub fn internal(code: &'static str, message: impl Into<String>, context: Value) -> Self {
        Self::Internal {
            code,
            message: message.into(),
            context,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Self::Validation { code, .. }
            | Self::Auth { code, .. }
            | Self::NotFound { code, .. }
            | Self::Conflict { code, .. }
            | Self::RateLimit { code, .. }
            | Self::Network { code, .. }
            | Self::Timeout { code, .. }
            | Self::Internal { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Validation { message, .. }
            | Self::Auth { message, .. }
            | Self::NotFound { message, .. }
            | Self::Conflict { message, .. }
            | Self::RateLimit { message, .. }
            | Self::Network { message, .. }
            | Self::Timeout { message, .. }
            | Self::Internal { message, .. } => message,
        }
    }

    pub fn context(&self) -> &Value {
        match self {
            Self::Validation { context, .. }
            | Self::Auth { context, .. }
            | Self::NotFound { context, .. }
            | Self::Conflict { context, .. }
            | Self::RateLimit { context, .. }
            | Self::Network { context, .. }
            | Self::Timeout { context, .. }
            | Self::Internal { context, .. } => context,
        }
    }

    pub fn status_code(&self) -> u16 {
        match self {
            Self::Validation { .. } => 400,
            Self::Auth { code, .. } if *code == "UNAUTHENTICATED" => 401,
            Self::Auth { .. } => 403,
            Self::NotFound { .. } => 404,
            Self::Conflict { .. } => 409,
            Self::RateLimit { .. } => 429,
            Self::Network { .. } => 503,
            Self::Timeout { .. } => 504,
            Self::Internal { .. } => 500,
        }
    }

    pub fn should_retry(&self) -> bool {
        matches!(
            self,
            Self::Network { .. } | Self::Timeout { .. } | Self::Internal { .. }
        )
    }

    pub fn with_context(self, key: &'static str, value: Value) -> Self {
        let mut context = self.context().clone();
        if !context.is_object() {
            context = json!({});
        }

        if let Some(map) = context.as_object_mut() {
            map.insert(key.to_owned(), value);
        }

        match self {
            Self::Validation { code, message, .. } => Self::Validation {
                code,
                message,
                context,
            },
            Self::Auth { code, message, .. } => Self::Auth {
                code,
                message,
                context,
            },
            Self::NotFound { code, message, .. } => Self::NotFound {
                code,
                message,
                context,
            },
            Self::Conflict { code, message, .. } => Self::Conflict {
                code,
                message,
                context,
            },
            Self::RateLimit { code, message, .. } => Self::RateLimit {
                code,
                message,
                context,
            },
            Self::Network { code, message, .. } => Self::Network {
                code,
                message,
                context,
            },
            Self::Timeout { code, message, .. } => Self::Timeout {
                code,
                message,
                context,
            },
            Self::Internal { code, message, .. } => Self::Internal {
                code,
                message,
                context,
            },
        }
    }
}
