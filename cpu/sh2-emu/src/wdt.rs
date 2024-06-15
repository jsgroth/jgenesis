//! SH7604 watchdog timer

use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub enum WatchdogTimerMode {
    #[default]
    Interval = 0,
    Watchdog = 1,
}

impl WatchdogTimerMode {
    fn from_bit(bit: bool) -> Self {
        if bit { Self::Watchdog } else { Self::Interval }
    }
}

fn clock_shift_select(byte: u8) -> u8 {
    match byte & 7 {
        // sysclk/2
        0 => 1,
        // sysclk/64
        1 => 6,
        // sysclk/128
        2 => 7,
        // sysclk/256
        3 => 8,
        // sysclk/512
        4 => 9,
        // sysclk/1024
        5 => 10,
        // sysclk/4096
        6 => 12,
        // sysclk/8192
        7 => 13,
        _ => unreachable!("value & 7 is always <= 7"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchdogTickEffect {
    None,
    Overflow,
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct WatchdogTimer {
    timer_counter: u8,
    mode: WatchdogTimerMode,
    enabled: bool,
    system_clock_shift: u8,
    system_clock_counter: u64,
    interval_overflow_flag: bool,
}

impl WatchdogTimer {
    pub fn new() -> Self {
        Self {
            timer_counter: 0,
            mode: WatchdogTimerMode::default(),
            enabled: false,
            system_clock_shift: clock_shift_select(0),
            system_clock_counter: 0,
            interval_overflow_flag: false,
        }
    }

    #[must_use]
    pub fn tick(&mut self, system_cycles: u64) -> WatchdogTickEffect {
        if !self.enabled {
            return WatchdogTickEffect::None;
        }

        self.system_clock_counter += system_cycles;
        let elapsed = self.system_clock_counter >> self.system_clock_shift;
        self.system_clock_counter &= (1 << self.system_clock_shift) - 1;

        let exceeds_byte = elapsed >= 256;
        let (counter, overflowed) = self.timer_counter.overflowing_add(elapsed as u8);
        self.timer_counter = counter;

        let overflow_flag = exceeds_byte || overflowed;
        self.interval_overflow_flag |= overflow_flag;

        if overflow_flag { WatchdogTickEffect::Overflow } else { WatchdogTickEffect::None }
    }

    // $FFFFFE80: WTCSR (Watchdog timer control/status) / WTCNT (Watchdog timer counter)
    // Upper byte determines which register is written to
    pub fn write_control(&mut self, value: u16) {
        log::debug!("Watchdog timer control write: {value:04X}");

        let [msb, lsb] = value.to_be_bytes();
        match msb {
            0x5A => self.write_wtcnt(lsb),
            0xA5 => self.write_wtcsr(lsb),
            _ => {
                log::warn!("Invalid watchdog timer write to $FFFFFE80: {value:04X}");
            }
        }
    }

    fn write_wtcnt(&mut self, value: u8) {
        self.timer_counter = value;

        log::debug!("WTCNT write: {value:02X}");
    }

    fn write_wtcsr(&mut self, value: u8) {
        self.interval_overflow_flag &= value.bit(7);
        self.mode = WatchdogTimerMode::from_bit(value.bit(6));
        self.enabled = value.bit(5);
        self.system_clock_shift = clock_shift_select(value);

        if !self.enabled {
            // Counter is reset when timer is disabled
            self.timer_counter = 0;
        }

        log::debug!("WTCSR write: {value:02X}");
        log::debug!("  Clear overflow flag: {}", !value.bit(7));
        log::debug!("  Timer mode: {:?}", self.mode);
        log::debug!("  Timer enabled: {}", self.enabled);
        log::debug!("  System clock divider: {}", 1 << self.system_clock_shift);

        if self.mode == WatchdogTimerMode::Watchdog {
            todo!("Watchdog timer mode not implemented")
        }
    }
}
