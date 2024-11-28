//! GBA hardware timers

use crate::apu::Apu;
use crate::control::{ControlRegisters, InterruptType};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
enum Prescaler {
    // clk/1 (16.777216 MHz)
    #[default]
    One = 0,
    // clk/64 (262.144 KHz)
    SixtyFour = 1,
    // clk/256 (65.536 KHz)
    TwoFiftySix = 2,
    // clk/1024 (16.384 KHz)
    TenTwentyFour = 3,
}

impl Prescaler {
    fn from_bits(bits: u16) -> Self {
        match bits & 3 {
            0 => Self::One,
            1 => Self::SixtyFour,
            2 => Self::TwoFiftySix,
            3 => Self::TenTwentyFour,
            _ => unreachable!("value & 3 is always <= 3"),
        }
    }

    const fn clock_shift(self) -> u32 {
        match self {
            Self::One => 0,
            Self::SixtyFour => 6,
            Self::TwoFiftySix => 8,
            Self::TenTwentyFour => 10,
        }
    }

    const fn cycles_mask(self) -> u64 {
        !((1 << self.clock_shift()) - 1)
    }
}

impl Display for Prescaler {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::One => write!(f, "clk/1"),
            Self::SixtyFour => write!(f, "clk/64"),
            Self::TwoFiftySix => write!(f, "clk/256"),
            Self::TenTwentyFour => write!(f, "clk/1024"),
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct Timer {
    counter: u16,
    reload_value: u16,
    prescaler: Prescaler,
    cascading: bool,
    irq_enabled: bool,
    enabled: bool,
    last_overflow_cycles: u64,
}

impl Timer {
    fn read_counter(&self, cycle_counter: u64) -> u16 {
        if !self.enabled || self.cascading {
            return self.counter;
        }

        let clock_shift = self.prescaler.clock_shift();
        let difference = (cycle_counter - self.last_overflow_cycles) >> clock_shift;

        self.counter + difference as u16
    }

    fn read_control(&self) -> u16 {
        (self.prescaler as u16)
            | (u16::from(self.cascading) << 2)
            | (u16::from(self.irq_enabled) << 6)
            | (u16::from(self.enabled) << 7)
    }

    fn write_control(&mut self, value: u16, timer_idx: usize, cycle_counter: u64) {
        let prev_prescaler = self.prescaler;
        let prev_cascading = self.cascading;
        let prev_enabled = self.enabled;

        self.prescaler = Prescaler::from_bits(value);
        self.cascading = timer_idx != 0 && value.bit(2);
        self.irq_enabled = value.bit(6);
        self.enabled = value.bit(7);

        if !prev_enabled && self.enabled {
            self.counter = self.reload_value;
            self.last_overflow_cycles = cycle_counter;
        }

        if !prev_cascading && self.cascading {
            self.counter += ((cycle_counter - self.last_overflow_cycles)
                >> prev_prescaler.clock_shift()) as u16;
        } else if prev_cascading && !self.cascading {
            self.last_overflow_cycles = cycle_counter;
        }

        log::trace!("TM{timer_idx}CNT_H write: {value:04X}");
        log::trace!("  Prescaler: {}", self.prescaler);
        log::trace!("  Cascading mode: {}", self.cascading);
        log::trace!("  IRQ enabled: {}", self.irq_enabled);
        log::trace!("  Timer enabled: {}", self.enabled);
    }

    fn next_overflow_cycles(&self) -> u64 {
        if !self.enabled || self.cascading {
            return u64::MAX;
        }

        let clocks_remaining = 0x10000 - u64::from(self.counter);
        let clock_shift = self.prescaler.clock_shift();
        ((self.last_overflow_cycles >> clock_shift) + clocks_remaining) << clock_shift
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Timers {
    timers: [Timer; 4],
    cycle_counter: u64,
    next_overflow_cycles: [u64; 4],
    min_next_overflow_cycles: u64,
}

impl Timers {
    pub fn new() -> Self {
        Self {
            timers: array::from_fn(|_| Timer::default()),
            cycle_counter: 0,
            next_overflow_cycles: [u64::MAX; 4],
            min_next_overflow_cycles: u64::MAX,
        }
    }

    pub fn tick(&mut self, cycles: u32, apu: &mut Apu, control: &mut ControlRegisters) {
        self.cycle_counter += u64::from(cycles);
        if self.cycle_counter < self.min_next_overflow_cycles {
            return;
        }

        for timer_idx in 0..4 {
            if self.cycle_counter < self.next_overflow_cycles[timer_idx] {
                continue;
            }

            loop {
                self.overflow_timer(timer_idx, apu, control);

                self.timers[timer_idx].last_overflow_cycles = self.next_overflow_cycles[timer_idx];
                self.next_overflow_cycles[timer_idx] =
                    self.timers[timer_idx].next_overflow_cycles();

                log::trace!(
                    "Next overflow for timer {timer_idx}: {}",
                    self.next_overflow_cycles[timer_idx]
                );

                if self.cycle_counter < self.next_overflow_cycles[timer_idx] {
                    break;
                }
            }
        }

        self.update_min_next_overflow_cycles();
    }

    fn overflow_timer(&mut self, timer_idx: usize, apu: &mut Apu, control: &mut ControlRegisters) {
        log::trace!("Overflowing timer {timer_idx}");

        self.timers[timer_idx].counter = self.timers[timer_idx].reload_value;

        if self.timers[timer_idx].irq_enabled {
            control.set_interrupt_flag(InterruptType::timer(timer_idx));
        }

        match timer_idx {
            0 => apu.timer_0_overflow(),
            1 => apu.timer_1_overflow(),
            _ => {}
        }

        if timer_idx < 3
            && self.timers[timer_idx + 1].enabled
            && self.timers[timer_idx + 1].cascading
        {
            if self.timers[timer_idx + 1].counter == 0xFFFF {
                self.overflow_timer(timer_idx + 1, apu, control);
            } else {
                self.timers[timer_idx + 1].counter += 1;
            }
        }
    }

    pub fn read_register(&self, address: u32) -> u16 {
        let timer_idx = ((address >> 2) & 3) as usize;
        if !address.bit(1) {
            self.timers[timer_idx].read_counter(self.cycle_counter)
        } else {
            self.timers[timer_idx].read_control()
        }
    }

    pub fn write_register(&mut self, address: u32, value: u16) {
        let timer_idx = ((address >> 2) & 3) as usize;
        if !address.bit(1) {
            self.timers[timer_idx].reload_value = value;
            log::trace!("TM{timer_idx}CNT_L write: {value:04X} (Timer reload value)");
        } else {
            self.timers[timer_idx].write_control(value, timer_idx, self.cycle_counter);
            self.next_overflow_cycles[timer_idx] = self.timers[timer_idx].next_overflow_cycles();
            self.update_min_next_overflow_cycles();
        }
    }

    fn update_min_next_overflow_cycles(&mut self) {
        self.min_next_overflow_cycles = self.next_overflow_cycles.into_iter().min().unwrap();
    }
}
