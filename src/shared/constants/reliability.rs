pub const MAX_EXTERNAL_RETRY_ATTEMPTS: usize = 3;
pub const MAX_NOTIFICATION_RETRY_ATTEMPTS: i32 = 3;
/// Base delay for exponential backoff: attempt 1 → 60 s, attempt 2 → 120 s, attempt 3 → 240 s.
pub const NOTIFICATION_RETRY_BASE_SECONDS: i64 = 60;
/// Cap so no retry is scheduled further than 1 hour out.
pub const NOTIFICATION_RETRY_MAX_SECONDS: i64 = 3_600;
pub const CIRCUIT_BREAKER_FAILURE_THRESHOLD: u32 = 5;
pub const CIRCUIT_BREAKER_OPEN_SECONDS: u64 = 60;
pub const PENDING_NOTIFICATION_BATCH_SIZE: i64 = 25;
/// Maximum number of Telegram send calls in flight at once.
/// Keeps us well below the 30 msg/s bot-wide Telegram rate limit.
pub const MAX_CONCURRENT_NOTIFICATION_DELIVERIES: usize = 5;
