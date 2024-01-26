//! S-RTC coprocessor, a Sharp real-time clock chip
//!
//! Used by Daikaijuu Monogatari II

use bincode::{Decode, Encode};
use jgenesis_common::timeutils;
use time::Weekday;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum ReadState {
    Ack,
    Digit { idx: u8 },
    End,
}

impl Default for ReadState {
    fn default() -> Self {
        Self::Ack
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
enum WriteState {
    Start,
    Command,
    Digit { idx: u8 },
    End,
}

impl Default for WriteState {
    fn default() -> Self {
        Self::Start
    }
}

trait WeekdayExt {
    fn to_srtc_u8(self) -> u8;
}

impl WeekdayExt for Weekday {
    fn to_srtc_u8(self) -> u8 {
        match self {
            Self::Sunday => 0,
            Self::Monday => 1,
            Self::Tuesday => 2,
            Self::Wednesday => 3,
            Self::Thursday => 4,
            Self::Friday => 5,
            Self::Saturday => 6,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SRtc {
    last_update_nanos: u128,
    nanos: u32,
    seconds: u8,
    minutes: u8,
    hours: u8,
    day: u8,
    month: u8,
    year: u8,
    century: u8,
    day_of_week: u8,
    read_state: ReadState,
    write_state: WriteState,
}

impl Default for SRtc {
    fn default() -> Self {
        Self::new()
    }
}

impl SRtc {
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_update_nanos: timeutils::current_time_nanos(),
            nanos: 0,
            seconds: 0,
            minutes: 0,
            hours: 0,
            day: 1,
            month: 1,
            year: 0,
            century: 9,
            day_of_week: 0,
            read_state: ReadState::default(),
            write_state: WriteState::default(),
        }
    }

    #[allow(clippy::missing_panics_doc)]
    #[inline]
    #[must_use]
    pub fn read(&mut self) -> u8 {
        log::trace!("S-RTC read");

        self.write_state = WriteState::default();

        self.update_time();

        match self.read_state {
            ReadState::Ack => {
                self.read_state = ReadState::Digit { idx: 0 };
                0x0F
            }
            ReadState::Digit { idx } => {
                let value = match idx {
                    0 => self.seconds % 10,
                    1 => self.seconds / 10,
                    2 => self.minutes % 10,
                    3 => self.minutes / 10,
                    4 => self.hours % 10,
                    5 => self.hours / 10,
                    6 => self.day % 10,
                    7 => self.day / 10,
                    8 => self.month,
                    9 => self.year % 10,
                    10 => self.year / 10,
                    11 => self.century,
                    12 => self.day_of_week,
                    _ => panic!("Invalid S-RTC timestamp index: {idx}"),
                };

                self.read_state = match idx {
                    12 => ReadState::End,
                    _ => ReadState::Digit { idx: idx + 1 },
                };

                log::trace!(
                    "S-RTC read, sending value {value:X} for idx {idx}; current state is {self:?}"
                );

                value
            }
            ReadState::End => {
                self.read_state = ReadState::Ack;
                0x0F
            }
        }
    }

    #[inline]
    pub fn write(&mut self, value: u8) {
        log::trace!("S-RTC write {value:X}");

        self.read_state = ReadState::default();

        self.update_time();

        // This is a 4-bit port
        let value = value & 0x0F;

        match self.write_state {
            WriteState::Start => {
                if value == 0x0E {
                    self.write_state = WriteState::Command;
                }
            }
            WriteState::Command => {
                match value {
                    0x04 => {
                        // Unknown command; possibly sets 24-hour mode?
                        self.write_state = WriteState::End;
                    }
                    0x00 => {
                        self.write_state = WriteState::Digit { idx: 0 };
                    }
                    _ => {}
                }
            }
            WriteState::Digit { idx } => {
                self.write_timestamp_digit(idx, value);

                self.write_state = match idx {
                    11 => WriteState::End,
                    _ => WriteState::Digit { idx: idx + 1 },
                };
            }
            WriteState::End => {
                if value == 0x0D {
                    self.write_state = WriteState::Start;
                }
            }
        }
    }

    pub fn reset_state(&mut self) {
        self.read_state = ReadState::default();
        self.write_state = WriteState::default();
    }

    fn write_timestamp_digit(&mut self, idx: u8, value: u8) {
        match idx {
            0 => {
                self.seconds = self.seconds / 10 * 10 + value;
            }
            1 => {
                self.seconds = 10 * value + (self.seconds % 10);
            }
            2 => {
                self.minutes = self.minutes / 10 * 10 + value;
            }
            3 => {
                self.minutes = 10 * value + (self.minutes % 10);
            }
            4 => {
                self.hours = self.hours / 10 * 10 + value;
            }
            5 => {
                self.hours = 10 * value + (self.hours % 10);
            }
            6 => {
                self.day = self.day / 10 * 10 + value;
                self.update_day_of_week();
            }
            7 => {
                self.day = 10 * value + (self.day % 10);
                self.update_day_of_week();
            }
            8 => {
                self.month = value;
                self.update_day_of_week();
            }
            9 => {
                self.year = self.year / 10 * 10 + value;
                self.update_day_of_week();
            }
            10 => {
                self.year = 10 * value + (self.year % 10);
                self.update_day_of_week();
            }
            11 => {
                self.century = value;
                self.update_day_of_week();
            }
            _ => panic!("Invalid S-RTC timestamp index: {idx}"),
        }

        log::trace!("S-RTC timestamp write, index {idx} value {value:X}; new time is {self:?}");
    }

    fn update_day_of_week(&mut self) {
        self.day_of_week =
            timeutils::day_of_week(self.day, self.month, four_digit_year(self.year, self.century))
                .to_srtc_u8();
    }

    fn update_time(&mut self) {
        let now_nanos = timeutils::current_time_nanos();
        let elapsed = now_nanos.saturating_sub(self.last_update_nanos);
        self.last_update_nanos = now_nanos;

        let new_nanos = u128::from(self.nanos) + elapsed;
        self.nanos = (new_nanos % 1_000_000_000) as u32;

        for _ in 0..new_nanos / 1_000_000_000 {
            self.increment_seconds();
        }
    }

    fn increment_seconds(&mut self) {
        self.seconds += 1;
        if self.seconds >= 60 {
            self.seconds = 0;
            self.increment_minutes();
        }
    }

    fn increment_minutes(&mut self) {
        self.minutes += 1;
        if self.minutes >= 60 {
            self.minutes = 0;
            self.increment_hours();
        }
    }

    fn increment_hours(&mut self) {
        self.hours += 1;
        if self.hours >= 24 {
            self.hours = 0;
            self.increment_day();
        }
    }

    fn increment_day(&mut self) {
        self.day += 1;
        self.day_of_week = (self.day_of_week + 1) % 7;

        if self.day > timeutils::days_in_month(self.month, self.year) {
            self.day = 1;
            self.increment_month();
        }
    }

    fn increment_month(&mut self) {
        self.month += 1;
        if self.month > 12 {
            self.month = 1;
            self.increment_year();
        }
    }

    fn increment_year(&mut self) {
        self.year += 1;
        if self.year > 99 {
            self.year = 0;
            self.century += 1;
        }
    }
}

fn four_digit_year(year: u8, century: u8) -> u16 {
    1000 + 100 * u16::from(century) + u16::from(year)
}
