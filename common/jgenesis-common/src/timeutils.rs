use cfg_if::cfg_if;
use std::thread;
use std::time::Duration;
use time::{Date, Month, Weekday};

/// Read the time since the Unix epoch in nanoseconds. Will return 0 if the system-reported time is
/// somehow before the Unix epoch.
///
/// Uses `SystemTime` on native platforms and `Date` in WASM.
#[must_use]
pub fn current_time_nanos() -> u128 {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let current_time_ms = js_sys::Date::now();
            (current_time_ms * 1_000_000.0) as u128
        } else {
            use std::time::SystemTime;

            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_nanos()
        }
    }
}

/// Sleep until at least the specified time. Returns the current time in nanoseconds after sleeping.
///
/// This implementation will try to sleep until 1ms before the target time and then it will busy
/// wait until the target time. This is to work around the fact that `thread::sleep()` only guarantees
/// that it will sleep _at least_ the specified duration, and it could sleep longer. Sleeping longer
/// seems to be particularly common on Windows.
///
/// If the current time is already past the target time, this function will return immediately
/// without sleeping.
#[inline]
#[allow(clippy::must_use_candidate)]
pub fn sleep_until(time_nanos: u128) -> u128 {
    loop {
        let now = current_time_nanos();
        if now >= time_nanos {
            return now;
        }

        let duration = Duration::from_nanos((time_nanos - now) as u64);
        if duration > Duration::from_millis(1) {
            thread::sleep(duration - Duration::from_millis(1));
        }
    }
}

/// Determine the number of days in the given month+year.
///
/// Leap years are accounted for, but only in that February is assumed to be 29 days in every 4th
/// year. Other leap year rules are intentionally not applied.
#[must_use]
pub fn days_in_month(month: u8, year: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            // This is not strictly accurate (it doesn't account for not divisible by 100 or divisible
            // by 400) but it matches what the RTC chips do
            if year % 4 == 0 { 29 } else { 28 }
        }
        _ => {
            log::error!("Invalid month: {month}, defaulting to 31 days in month");
            31
        }
    }
}

/// Determine the weekday of a given date. Day and month should both start at 1, not 0.
#[must_use]
pub fn day_of_week(day: u8, month: u8, year: u16) -> Weekday {
    match Date::from_calendar_date(year.into(), convert_month(month), day) {
        Ok(date) => date.weekday(),
        Err(err) => {
            log::error!("Invalid date (day={day}, month={month}, year={year}): {err}");
            Weekday::Sunday
        }
    }
}

fn convert_month(month: u8) -> Month {
    use Month::*;

    match month {
        1 => January,
        2 => February,
        3 => March,
        4 => April,
        5 => May,
        6 => June,
        7 => July,
        8 => August,
        9 => September,
        10 => October,
        11 => November,
        12 => December,
        _ => {
            log::error!("Invalid month: {month} defaulting to January");
            January
        }
    }
}
