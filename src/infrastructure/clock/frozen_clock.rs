//! Deterministic `Clock` implementation used from integration tests and
//! bootstrap smoke tests.  Time only advances when the test explicitly asks
//! it to — crucial for reproducing SLA, notification, and recurrence logic
//! without flaky `sleep`-based races.

use std::sync::{Arc, RwLock};

use chrono::{DateTime, Duration, NaiveDate, Utc};

use crate::application::ports::services::Clock;

#[derive(Clone)]
pub struct FrozenClock {
    inner: Arc<RwLock<DateTime<Utc>>>,
}

impl FrozenClock {
    pub fn new(now: DateTime<Utc>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(now)),
        }
    }

    pub fn set(&self, new_now: DateTime<Utc>) {
        let mut guard = self.inner.write().expect("frozen clock lock poisoned");
        *guard = new_now;
    }

    pub fn advance(&self, delta: Duration) {
        let mut guard = self.inner.write().expect("frozen clock lock poisoned");
        *guard += delta;
    }
}

impl Clock for FrozenClock {
    fn now_utc(&self) -> DateTime<Utc> {
        *self.inner.read().expect("frozen clock lock poisoned")
    }

    fn today_utc(&self) -> NaiveDate {
        self.now_utc().date_naive()
    }
}

#[cfg(test)]
mod tests {
    use super::FrozenClock;
    use crate::application::ports::services::Clock;
    use chrono::{Duration, TimeZone, Utc};

    #[test]
    fn given_frozen_clock_when_queried_twice_then_returns_same_time() {
        let t = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let clock = FrozenClock::new(t);

        let a = clock.now_utc();
        let b = clock.now_utc();

        assert_eq!(a, b);
        assert_eq!(a, t);
    }

    #[test]
    fn given_advance_when_called_then_time_moves_forward() {
        let t0 = Utc.with_ymd_and_hms(2026, 1, 1, 12, 0, 0).unwrap();
        let clock = FrozenClock::new(t0);

        clock.advance(Duration::minutes(30));

        assert_eq!(clock.now_utc() - t0, Duration::minutes(30));
    }
}
