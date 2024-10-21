//! Cycle counting and wait state tracking for the Genesis hardware

use bincode::{Decode, Encode};
use std::num::{NonZeroU32, NonZeroU64};
use std::{cmp, mem};

pub const NATIVE_M68K_DIVIDER: u64 = 7;
pub const Z80_DIVIDER: u64 = 15;
pub const YM2612_DIVIDER: u64 = 7;
pub const PSG_DIVIDER: u64 = 15;

// Multiplication and division instructions all take at least 40 CPU cycles and don't access the bus
// while executing
const LONG_68K_INSTRUCTION_THRESHOLD: u64 = 40 * NATIVE_M68K_DIVIDER;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct CycleCounters<const REFRESH_INTERVAL: u64> {
    // Store divider as both u64 and u32 for better codegen when doing u32 division
    pub m68k_divider: NonZeroU64,
    pub m68k_divider_u32: NonZeroU32,
    pub max_wait_cpu_cycles: u32,
    pub m68k_wait_cpu_cycles: u32,
    pub z80_mclk_counter: u64,
    pub z80_wait_mclk_cycles: u64,
    pub z80_odd_access: bool,
    pub ym2612_mclk_counter: u64,
    pub psg_mclk_counter: u64,
    pub refresh_mclk_counter: u64,
}

fn max_wait_cpu_cycles(m68k_divider: NonZeroU64) -> u32 {
    // Executing for too many cycles at a time breaks assumptions in the VDP code, checked via an
    // assert in Vdp::tick()
    const MAX_WAIT_MCLK_CYCLES: u32 = 1225;

    MAX_WAIT_MCLK_CYCLES / m68k_divider.get() as u32
}

impl<const REFRESH_INTERVAL: u64> CycleCounters<REFRESH_INTERVAL> {
    #[inline]
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn new(m68k_divider: NonZeroU64) -> Self {
        let m68k_divider_u32 = NonZeroU32::new(m68k_divider.get() as u32).unwrap();
        let max_wait_cpu_cycles = max_wait_cpu_cycles(m68k_divider);

        Self {
            m68k_divider,
            m68k_divider_u32,
            max_wait_cpu_cycles,
            m68k_wait_cpu_cycles: 0,
            z80_mclk_counter: 0,
            z80_wait_mclk_cycles: 0,
            z80_odd_access: false,
            ym2612_mclk_counter: 0,
            psg_mclk_counter: 0,
            refresh_mclk_counter: 0,
        }
    }

    #[inline]
    #[allow(clippy::missing_panics_doc)]
    pub fn update_m68k_divider(&mut self, m68k_divider: NonZeroU64) {
        self.m68k_divider = m68k_divider;
        self.m68k_divider_u32 = NonZeroU32::new(m68k_divider.get() as u32).unwrap();
        self.max_wait_cpu_cycles = max_wait_cpu_cycles(m68k_divider);

        self.m68k_wait_cpu_cycles = cmp::min(self.m68k_wait_cpu_cycles, self.max_wait_cpu_cycles);
    }

    #[inline]
    pub fn take_m68k_wait_cpu_cycles(&mut self) -> u32 {
        mem::take(&mut self.m68k_wait_cpu_cycles)
    }

    #[inline]
    #[must_use]
    pub fn record_68k_instruction(&mut self, m68k_cycles: u32, was_mul_or_div: bool) -> u64 {
        let mut mclk_cycles = u64::from(m68k_cycles) * self.m68k_divider.get();

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
