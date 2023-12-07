use cfg_if::cfg_if;
use time::{Date, Month, Weekday};

pub fn current_time_nanos() -> u128 {
    cfg_if! {
        if #[cfg(target_arch = "wasm32")] {
            let current_time_ms = js_sys::Date::now();
            (current_time_ms * 1_000_000.0) as u128
        } else {
            use std::time::SystemTime;

            SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos()
        }
    }
}

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
