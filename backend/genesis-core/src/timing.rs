//! Cycle counting and wait state tracking for the Genesis hardware

use bincode::{Decode, Encode};
use std::{cmp, mem};

pub const M68K_DIVIDER: u64 = 7;
pub const Z80_DIVIDER: u64 = 15;
pub const YM2612_DIVIDER: u64 = 7;
pub const PSG_DIVIDER: u64 = 15;

// Multiplication and division instructions all take at least 40 CPU cycles and don't access the bus
// while executing
const LONG_68K_INSTRUCTION_THRESHOLD: u64 = 40 * M68K_DIVIDER;

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
pub struct CycleCounters<const REFRESH_INTERVAL: u64> {
    pub m68k_wait_cpu_cycles: u32,
    pub z80_mclk_counter: u64,
    pub z80_wait_mclk_cycles: u64,
    pub z80_odd_access: bool,
    pub ym2612_mclk_counter: u64,
    pub psg_mclk_counter: u64,
    pub refresh_mclk_counter: u64,
}

impl<const REFRESH_INTERVAL: u64> CycleCounters<REFRESH_INTERVAL> {
    #[inline]
    pub fn take_m68k_wait_cpu_cycles(&mut self) -> u32 {
        mem::take(&mut self.m68k_wait_cpu_cycles)
    }

    #[inline]
    #[must_use]
    pub fn record_68k_instruction(&mut self, m68k_cycles: u32, was_mul_or_div: bool) -> u64 {
        let mut mclk_cycles = u64::from(m68k_cycles) * M68K_DIVIDER;

        // Track memory refresh delay, which stalls the 68000 for roughly 2 out of every 128 mclk cycles
        // (at least on the standalone Genesis)
        // Clue and Super Airwolf depend on this or they will have graphical glitches, Clue in the
        // main menu and Super Airwolf in the intro
        self.refresh_mclk_counter += mclk_cycles;
        if was_mul_or_div && mclk_cycles >= LONG_68K_INSTRUCTION_THRESHOLD {
            // Only incur memory refresh delay once for multiplication/division instructions
            mclk_cycles += 2;
            self.refresh_mclk_counter %= REFRESH_INTERVAL - 2;
        } else {
            while self.refresh_mclk_counter >= REFRESH_INTERVAL - 2 {
                self.refresh_mclk_counter -= REFRESH_INTERVAL - 2;
                mclk_cycles += 2;
            }
        }

        self.increment_mclk_counters(mclk_cycles);

        mclk_cycles
    }

    #[inline]
    pub fn increment_mclk_counters(&mut self, mclk_cycles: u64) {
        self.z80_mclk_counter += mclk_cycles;
        self.ym2612_mclk_counter += mclk_cycles;
        self.psg_mclk_counter += mclk_cycles;

        let z80_wait_elapsed = cmp::min(self.z80_mclk_counter, self.z80_wait_mclk_cycles);
        self.z80_mclk_counter -= z80_wait_elapsed;
        self.z80_wait_mclk_cycles -= z80_wait_elapsed;
    }

    #[inline]
    pub fn record_z80_68k_bus_access(&mut self) {
        // Each time the Z80 accesses the 68K bus, the Z80 is stalled for on average 3.3 Z80 cycles (= 49.5 mclk cycles)
        // and the 68K is stalled for on average 11 68K cycles
        self.m68k_wait_cpu_cycles = 11;
        self.z80_wait_mclk_cycles = 49 + u64::from(self.z80_odd_access);
        self.z80_odd_access = !self.z80_odd_access;
    }

    #[inline]
    #[must_use]
    pub fn should_tick_z80(&self) -> bool {
        self.z80_mclk_counter >= Z80_DIVIDER
    }

    #[inline]
    pub fn decrement_z80(&mut self) {
        self.z80_mclk_counter -= Z80_DIVIDER;
    }

    #[inline]
    #[must_use]
    pub fn should_tick_ym2612(&self) -> bool {
        self.ym2612_mclk_counter >= YM2612_DIVIDER
    }

    #[inline]
    pub fn decrement_ym2612(&mut self) {
        self.ym2612_mclk_counter -= YM2612_DIVIDER;
    }

    #[inline]
    #[must_use]
    pub fn should_tick_psg(&self) -> bool {
        self.psg_mclk_counter >= PSG_DIVIDER
    }

    #[inline]
    pub fn decrement_psg(&mut self) {
        self.psg_mclk_counter -= PSG_DIVIDER;
    }
}

pub type GenesisCycleCounters = CycleCounters<128>;
