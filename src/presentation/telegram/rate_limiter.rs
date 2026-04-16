use std::num::NonZeroU32;
use std::sync::Arc;

use governor::{DefaultKeyedRateLimiter, Quota};

#[derive(Clone)]
pub struct TelegramRateLimiter {
    limiter: Arc<DefaultKeyedRateLimiter<u64>>,
}

impl TelegramRateLimiter {
    pub fn new() -> Self {
        let quota = Quota::per_minute(NonZeroU32::new(20).unwrap_or(NonZeroU32::MIN));
        Self {
            limiter: Arc::new(DefaultKeyedRateLimiter::<u64>::keyed(quota)),
        }
    }

    pub fn check(&self, actor_key: u64) -> bool {
        self.limiter.check_key(&actor_key).is_ok()
    }
}

impl Default for TelegramRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}
