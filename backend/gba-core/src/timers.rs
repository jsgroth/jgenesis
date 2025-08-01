use crate::apu::Apu;
use crate::dma::DmaState;
use crate::interrupts::{InterruptRegisters, InterruptType};
use bincode::{Decode, Encode};
use jgenesis_common::num::GetBit;
use std::array;

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

        let increment = if self.cascading {
            prev_overflowed
        } else {
            current_cycles & ((1 << self.clock_shift) - 1) == 0
        };

        let overflowed;
        (self.counter, overflowed) = self.counter.overflowing_add(increment.into());

        if overflowed {
            self.counter = self.reload;

            if self.irq_enabled {
                interrupts.set_flag(InterruptType::TIMER[self.idx as usize]);
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
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct Timers {
    timers: [Timer; 4],
    cycles: u64,
}

impl Timers {
    pub fn new() -> Self {
        Self { timers: array::from_fn(|i| Timer::new(i as u8)), cycles: 0 }
    }

    pub fn step_to(
        &mut self,
        cycles: u64,
        apu: &mut Apu,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) {
        // TODO this is extremely slow; optimize
        while self.cycles < cycles {
            self.cycles += 1;

            let mut overflowed = false;
            for (i, timer) in self.timers.iter_mut().enumerate() {
                overflowed = timer.tick(overflowed, self.cycles, interrupts);

                if overflowed && i <= 1 {
                    apu.handle_timer_overflow(i, self.cycles, dma);
                }
            }
        }
    }

    pub fn read_register(
        &mut self,
        address: u32,
        cycles: u64,
        apu: &mut Apu,
        dma: &mut DmaState,
        interrupts: &mut InterruptRegisters,
    ) -> u16 {
        log::trace!("Timer read {address:08X} at cycles {cycles}");

        let timer_idx = (address >> 2) & 3;

        if !address.bit(1) {
            self.step_to(cycles, apu, dma, interrupts);
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

        self.step_to(cycles, apu, dma, interrupts);

        let timer_idx = (address >> 2) & 3;

        if !address.bit(1) {
            self.timers[timer_idx as usize].pending_reload_write = Some(value);
        } else {
            self.timers[timer_idx as usize].pending_control_write = Some(value);
        }
    }
}
