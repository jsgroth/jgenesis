//! YM2612 timers

use bincode::{Decode, Encode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerTickEffect {
    None,
    Overflowed,
}

pub struct TimerControl {
    pub enabled: bool,
    pub overflow_flag_enabled: bool,
    pub clear_overflow_flag: bool,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct TimerA {
    enabled: bool,
    enabled_next: bool,
    overflow_flag_enabled: bool,
    overflow_flag: bool,
    interval: u16,
    counter: u16,
}

impl TimerA {
    pub fn new() -> Self {
        Self {
            enabled: false,
            enabled_next: false,
            overflow_flag_enabled: false,
            overflow_flag: false,
            interval: 0,
            counter: 0,
        }
    }

    pub fn tick(&mut self) -> TimerTickEffect {
        // Timer A counter is 10-bit in actual hardware
        const OVERFLOW: u16 = 1024;

        if !self.enabled {
            if self.enabled_next {
                self.enabled = true;
                self.counter = self.interval;
            }
            return TimerTickEffect::None;
        }

        self.enabled = self.enabled_next;

        self.counter += 1;
        if self.counter == OVERFLOW {
            self.overflow_flag |= self.overflow_flag_enabled;
            self.counter = self.interval;
            TimerTickEffect::Overflowed
        } else {
            TimerTickEffect::None
        }
    }

    pub fn overflow_flag(&self) -> bool {
        self.overflow_flag
    }

    pub fn interval(&self) -> u16 {
        self.interval
    }

    pub fn write_control(
        &mut self,
        TimerControl { enabled, overflow_flag_enabled, clear_overflow_flag }: TimerControl,
    ) {
        self.enabled_next = enabled;
        self.overflow_flag_enabled = overflow_flag_enabled;
        self.overflow_flag &= !clear_overflow_flag;
    }

    pub fn write_interval_high(&mut self, value: u8) {
        self.interval = (self.interval & 3) | (u16::from(value) << 2);
    }

    pub fn write_interval_low(&mut self, value: u8) {
        self.interval = (self.interval & !3) | u16::from(value & 3);
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct TimerB {
    enabled: bool,
    enabled_next: bool,
    overflow_flag_enabled: bool,
    overflow_flag: bool,
    pub interval: u8,
    counter: u8,
    divider: u8,
}

impl TimerB {
    // Timer B counter increments once per 16 samples
    const DIVIDER: u8 = 16;

    pub fn new() -> Self {
        Self {
            enabled: false,
            enabled_next: false,
            overflow_flag_enabled: false,
            overflow_flag: false,
            interval: 0,
            counter: 0,
            divider: Self::DIVIDER,
        }
    }

    pub fn tick(&mut self) {
        self.divider -= 1;
        if self.divider == 0 {
            self.divider = Self::DIVIDER;

            if self.enabled {
                let overflowed;
                (self.counter, overflowed) = self.counter.overflowing_add(1);
                if overflowed {
                    self.overflow_flag |= self.overflow_flag_enabled;
                    self.counter = self.interval;
                }
            }
        }

        if !self.enabled && self.enabled_next {
            self.counter = self.interval;
        }
        self.enabled = self.enabled_next;
    }

    pub fn overflow_flag(&self) -> bool {
        self.overflow_flag
    }

    pub fn write_control(
        &mut self,
        TimerControl { enabled, overflow_flag_enabled, clear_overflow_flag }: TimerControl,
    ) {
        self.enabled_next = enabled;
        self.overflow_flag_enabled = overflow_flag_enabled;
        self.overflow_flag &= !clear_overflow_flag;
    }
}
