use bincode::{Decode, Encode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_common::timeutils;

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
struct RtcTime {
    nanos: u32,
    seconds: u8,
    minutes: u8,
    hours: u8,
    days: u16,
    day_overflow: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Mbc3Rtc {
    current_time: RtcTime,
    latched_time: RtcTime,
    last_update_nanos: u128,
    last_latch_write: u8,
    halted: bool,
}

impl Mbc3Rtc {
    pub fn new() -> Self {
        Self::new_from_current_time(RtcTime::default(), timeutils::current_time_nanos())
    }

    fn new_from_current_time(current_time: RtcTime, last_update_nanos: u128) -> Self {
        Self {
            current_time,
            latched_time: RtcTime::default(),
            last_update_nanos,
            last_latch_write: 0xFF,
            halted: false,
        }
    }

    pub fn read_register(&self, register: u8) -> u8 {
        match register {
            0x08 => self.latched_time.seconds,
            0x09 => self.latched_time.minutes,
            0x0A => self.latched_time.hours,
            0x0B => self.latched_time.days.lsb(),
            0x0C => {
                (self.latched_time.days.msb() & 0x01)
                    | (u8::from(self.halted) << 6)
                    | (u8::from(self.latched_time.day_overflow) << 7)
            }
            _ => 0xFF,
        }
    }

    pub fn write_register(&mut self, register: u8, value: u8) {
        match register {
            0x08 => {
                self.current_time.seconds = value % 60;
            }
            0x09 => {
                self.current_time.minutes = value % 60;
            }
            0x0A => {
                self.current_time.hours = value % 24;
            }
            0x0B => {
                self.current_time.days.set_lsb(value);
            }
            0x0C => {
                self.current_time.days.set_msb(value & 0x01);
                self.halted = value.bit(6);
                self.current_time.day_overflow = value.bit(7);
            }
            _ => {}
        }
    }

    pub fn write_latch(&mut self, value: u8) {
        // Writing $00 then $01 updates the RTC latch
        if self.last_latch_write == 0x00 && value == 0x01 {
            self.latched_time = self.current_time;
            log::trace!("RTC latched to {:?}", self.latched_time);
        }

        self.last_latch_write = value;
    }

    pub fn update_time(&mut self) {
        let current_time_nanos = timeutils::current_time_nanos();
        if current_time_nanos < self.last_update_nanos {
            log::error!(
                "Time has gone backwards; last update was at {} ns, current time is {current_time_nanos} ns",
                self.last_update_nanos
            );
            self.last_update_nanos = current_time_nanos;
            return;
        }

        if self.halted {
            self.last_update_nanos = current_time_nanos;
            return;
        }

        let elapsed_nanos = current_time_nanos - self.last_update_nanos;
        self.last_update_nanos = current_time_nanos;

        let new_nanos = u128::from(self.current_time.nanos) + elapsed_nanos;
        self.current_time.nanos = (new_nanos % 1_000_000_000) as u32;
        if new_nanos < 1_000_000_000 {
            return;
        }

        let new_seconds = u64::from(self.current_time.seconds) + (new_nanos / 1_000_000_000) as u64;
        self.current_time.seconds = (new_seconds % 60) as u8;
        if new_seconds < 60 {
            return;
        }

        let new_minutes = u64::from(self.current_time.minutes) + new_seconds / 60;
        self.current_time.minutes = (new_minutes % 60) as u8;
        if new_minutes < 60 {
            return;
        }

        let new_hours = u64::from(self.current_time.hours) + new_minutes / 60;
        self.current_time.hours = (new_hours % 24) as u8;
        if new_hours < 24 {
            return;
        }

        let new_days = u64::from(self.current_time.days) + new_hours / 24;
        self.current_time.days = (new_days % 512) as u16;
        if new_days >= 512 {
            self.current_time.day_overflow = true;
        }
    }
}
