use std::future::Future;
use std::time::Duration;

use tokio::time::sleep;

use crate::domain::errors::AppResult;
use crate::shared::constants::reliability::MAX_EXTERNAL_RETRY_ATTEMPTS;

pub async fn retry_with_backoff<F, Fut, T>(mut operation: F) -> AppResult<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = AppResult<T>>,
{
    let mut last_error = None;
    let mut delay = Duration::from_millis(250);

    for attempt in 1..=MAX_EXTERNAL_RETRY_ATTEMPTS {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(error) if error.should_retry() && attempt < MAX_EXTERNAL_RETRY_ATTEMPTS => {
                last_error = Some(error);
                sleep(delay).await;
                delay = delay.saturating_mul(2);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        crate::domain::errors::AppError::internal(
            "RETRY_STATE_INVALID",
            "Retry loop finished without preserving the last error",
            serde_json::json!({}),
        )
    }))
}
