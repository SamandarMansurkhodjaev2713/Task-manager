use chrono::{DateTime, NaiveDate, Utc};

use crate::application::ports::services::Clock;

pub struct SystemClock;

impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }

    fn today_utc(&self) -> NaiveDate {
        Utc::now().date_naive()
    }
}
