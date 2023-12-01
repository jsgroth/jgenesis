use cfg_if::cfg_if;

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
            if year % 4 == 0 {
                29
            } else {
                28
            }
        }
        _ => {
            log::error!("Invalid month: {month}, defaulting to 31 days in month");
            31
        }
    }
}
