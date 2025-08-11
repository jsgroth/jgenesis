use crate::apu::Apu;
use crate::dma::DmaState;
use crate::interrupts::{InterruptRegisters, InterruptType};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::{array, cmp};

#[derive(Debug, Clone, Encode, Decode)]
struct Timer {
    idx: u8,
    enabled: bool,
    counter: u16,
    reload: u16,
    clock_shift: u8,
    cascading: bool,
    irq_enabled: bool,
    pending_reload_write: Option<u16>,
    pending_control_write: Option<u16>,
    just_enabled: bool,
}

impl Timer {
    fn new(idx: u8) -> Self {
        Self {
            idx,
            enabled: false,
            counter: 0,
            reload: 0,
            clock_shift: 0,
            cascading: false,
            irq_enabled: false,
            pending_reload_write: None,
            pending_control_write: None,
            just_enabled: false,
        }
    }

    fn tick(
        &mut self,
        prev_overflowed: bool,
        prev_cycles: u64,
        current_cycles: u64,
        interrupts: &mut InterruptRegisters,
    ) -> bool {
        if !self.enabled {
            self.apply_pending_writes();
            return false;
        }

        if self.just_enabled {
            self.just_enabled = false;
            self.counter = self.reload;
            self.apply_pending_writes();

            return false;
        }

        let increment: u64 = if self.cascading {
            prev_overflowed.into()
        } else {
            (current_cycles >> self.clock_shift) - (prev_cycles >> self.clock_shift)
        };

        let mut overflowed;
        (self.counter, overflowed) = self.counter.overflowing_add(increment as u16);
        overflowed |= increment >= u64::from(u16::MAX);

        if overflowed {
            self.counter = self.reload;

            if self.irq_enabled {
                interrupts.set_flag(InterruptType::TIMER[self.idx as usize], current_cycles);
            }
        }

        self.apply_pending_writes();

        overflowed
    }

    fn apply_pending_writes(&mut self) {
        if let Some(reload) = self.pending_reload_write.take() {
            self.apply_reload_write(reload);
        }

        if let Some(control) = self.pending_control_write.take() {
            self.apply_control_write(control);
        }
    }

    fn apply_reload_write(&mut self, value: u16) {
        self.reload = value;

        log::trace!("TM{}CNT_L write: {value:04X} (reload value)", self.idx);
    }

    fn apply_control_write(&mut self, value: u16) {
        const CLOCK_SHIFTS: [u8; 4] = [
            0,  // Divider 1 (16777216 Hz)
            6,  // Divider 64 (262144 Hz)
            8,  // Divider 256 (65536 Hz)
            10, // Divider 1024 (16384 Hz)
        ];

        self.clock_shift = CLOCK_SHIFTS[(value & 3) as usize];
        self.cascading = self.idx != 0 && value.bit(2);
        self.irq_enabled = value.bit(6);

        let prev_enabled = self.enabled;
        self.enabled = value.bit(7);

        self.just_enabled = !prev_enabled && self.enabled;

        log::trace!("TM{}CNT_H write: {value:04X}", self.idx);
        log::trace!("  Prescaler divider: {}", 1 << self.clock_shift);
        log::trace!("  Cascading: {}", self.cascading);
        log::trace!("  IRQ enabled: {}", self.irq_enabled);
        log::trace!("  Timer enabled: {}", self.enabled);
    }

    fn read_control(&self) -> u16 {
        let divider_bits = if self.clock_shift == 0 { 0 } else { self.clock_shift / 2 - 2 };

        u16::from(divider_bits)
            | (u16::from(self.cascading) << 2)
            | (u16::from(self.irq_enabled) << 6)
            | (u16::from(self.enabled) << 7)
    }

    fn next_event_cycles(&self, cycles: u64) -> Option<u64> {
        if self.pending_reload_write.is_some()
            || self.pending_control_write.is_some()
            || self.just_enabled
        {
            // Force an update on the next cycle after register writes
            return Some(cycles + 1);
        }

        if !self.enabled || self.cascading {
            // Disabled timers never overflow, and cascading timers can only overflow when another
            // timer overflows
            return None;
        }

        let increments_until_overflow = 0x10000 - u64::from(self.counter);
        let mut cycles_until_overflow = increments_until_overflow << self.clock_shift;
        cycles_until_overflow -= cycles & ((1 << self.clock_shift) - 1);

        Some(cycles + cycles_until_overflow)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Timers {
    timers: [Timer; 4],
    cycles: u64,
    next_overflow_cycles: u64,
}

impl Timers {
    pub fn new() -> Self {
        Self {
            timers: array::from_fn(|i| Timer::new(i as u8)),
            cycles: 0,
            next_overflow_cycles: u64::MAX,
        }
    }

    pub fn step_to(
        &mut self,
        cycles: u64,
        apu: &mut Apu,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) {
        if cycles < self.next_overflow_cycles {
            return;
        }

        self.step_to_internal(cycles, apu, dma, interrupts);
    }

    fn step_to_internal(
        &mut self,
        cycles: u64,
        apu: &mut Apu,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) {
        while self.cycles < cycles {
            let tick_cycles = cmp::min(self.next_overflow_cycles, cycles);

            let mut overflowed = false;
            for (i, timer) in self.timers.iter_mut().enumerate() {
                overflowed = timer.tick(overflowed, self.cycles, tick_cycles, interrupts);

                if overflowed && i <= 1 {
                    apu.handle_timer_overflow(i, tick_cycles, dma);
                }
            }

            self.cycles = tick_cycles;
            self.update_next_overflow_cycles();
        }
    }

    fn update_next_overflow_cycles(&mut self) {
        self.next_overflow_cycles = self
            .timers
            .iter()
            .filter_map(|timer| timer.next_event_cycles(self.cycles))
            .min()
            .unwrap_or(u64::MAX);
    }

    pub fn read_register(
        &mut self,
        address: u32,
        cycles: u64,
        apu: &mut Apu,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) -> u16 {
        let timer_idx = (address >> 2) & 3;

        if !address.bit(1) {
            self.step_to_internal(cycles, apu, dma, interrupts);
            log::trace!(
                "Timer read {address:08X} at cycles {cycles}, counter {:04X}",
                self.timers[timer_idx as usize].counter
            );
            self.timers[timer_idx as usize].counter
        } else {
            self.timers[timer_idx as usize].read_control()
        }
    }

    pub fn write_register(
        &mut self,
        address: u32,
        value: u16,
        cycles: u64,
        apu: &mut Apu,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) {
        log::trace!("Timer write {address:08X} {value:04X} at cycles {cycles}");

        self.step_to_internal(cycles, apu, dma, interrupts);

        let timer_idx = (address >> 2) & 3;

        if !address.bit(1) {
            self.timers[timer_idx as usize].pending_reload_write = Some(value);
        } else {
            self.timers[timer_idx as usize].pending_control_write = Some(value);
        }

        self.update_next_overflow_cycles();
    }
}
