use std::time::Duration;

use tokio::sync::Mutex;

use crate::domain::errors::{AppError, AppResult};
use crate::shared::constants::reliability::{
    CIRCUIT_BREAKER_FAILURE_THRESHOLD, CIRCUIT_BREAKER_OPEN_SECONDS,
};

#[derive(Debug)]
pub struct CircuitBreaker {
    state: Mutex<CircuitBreakerState>,
}

#[derive(Debug, Default)]
struct CircuitBreakerState {
    consecutive_failures: u32,
    opened_until: Option<std::time::Instant>,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(CircuitBreakerState::default()),
        }
    }

    pub async fn ensure_closed(&self, service_name: &'static str) -> AppResult<()> {
        let state = self.state.lock().await;
        if let Some(opened_until) = state.opened_until {
            if std::time::Instant::now() < opened_until {
                return Err(AppError::network(
                    "CIRCUIT_BREAKER_OPEN",
                    format!("{service_name} circuit breaker is open"),
                    serde_json::json!({ "service": service_name }),
                ));
            }
        }
        Ok(())
    }

    pub async fn record_success(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_failures = 0;
        state.opened_until = None;
    }

    pub async fn record_failure(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);

        if state.consecutive_failures >= CIRCUIT_BREAKER_FAILURE_THRESHOLD {
            state.opened_until =
                Some(std::time::Instant::now() + Duration::from_secs(CIRCUIT_BREAKER_OPEN_SECONDS));
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}
