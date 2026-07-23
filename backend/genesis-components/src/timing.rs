//! Cycle counting and wait state tracking for the Genesis hardware

use crate::vdp;
use crate::vdp::VdpTickEffect;
use crate::ym2612::Ym2612;
use bincode::{Decode, Encode};
use std::num::{NonZeroU32, NonZeroU64};
use std::{cmp, mem};

pub const NATIVE_M68K_DIVIDER: u64 = genesis_config::NATIVE_M68K_DIVIDER;
pub const Z80_DIVIDER: u64 = 15;
pub const YM2612_DIVIDER: u64 = 7 * 6;
pub const PSG_DIVIDER: u64 = 15;

// Sync the YM2612 at least once per scanline
pub const MAX_YM2612_LAG_MCLK: u64 = vdp::MCLK_CYCLES_PER_SCANLINE;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct CycleCounters<const REFRESH_INTERVAL: u32> {
    // Store divider as both u64 and u32 for better codegen when doing u32 division
    pub m68k_divider: NonZeroU64,
    pub m68k_divider_u32: NonZeroU32,
    pub max_wait_cpu_cycles: u32,
    pub m68k_wait_cpu_cycles: u32,
    pub m68k_wait_counter: u8,
    pub m68k_mclk_cycles: u64,
    pub z80_mclk_cycles: u64,
    pub ym2612_mclk_cycles: u64,
    pub last_ym2612_drain_mclk: u64,
    pub psg_mclk_cycles: u64,
    pub m68k_refresh_counter: u32,
    pub vdp_owns_bus: bool,
    pub z80_halt: bool,
}

fn max_wait_cpu_cycles(m68k_divider: NonZeroU64) -> u32 {
    // Executing for too many cycles at a time breaks assumptions in the VDP code, checked via an
    // assert in Vdp::tick()
    const MAX_WAIT_MCLK_CYCLES: u32 = 1225;

    MAX_WAIT_MCLK_CYCLES / m68k_divider.get() as u32
}

impl<const REFRESH_INTERVAL: u32> CycleCounters<REFRESH_INTERVAL> {
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
            m68k_wait_counter: 0,
            m68k_mclk_cycles: 0,
            z80_mclk_cycles: 0,
            ym2612_mclk_cycles: 0,
            last_ym2612_drain_mclk: 0,
            psg_mclk_cycles: 0,
            m68k_refresh_counter: 0,
            vdp_owns_bus: false,
            z80_halt: false,
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
        if self.m68k_wait_cpu_cycles > self.max_wait_cpu_cycles {
            self.m68k_wait_cpu_cycles -= self.max_wait_cpu_cycles;
            return self.max_wait_cpu_cycles;
        }

        mem::take(&mut self.m68k_wait_cpu_cycles)
    }

    #[inline]
    #[must_use]
    pub fn record_68k_instruction(
        &mut self,
        m68k_pc: u32,
        m68k_cycles: u32,
        m68k_wait: bool,
        vdp_owns_bus: bool,
    ) -> u64 {
        const REGION_WAIT_CYCLES: [u32; 8] = [
            2, // $000000-$1FFFFF
            2, // $200000-$3FFFFF
            2, // $400000-$5FFFFF
            2, // $600000-$7FFFFF
            0, // $800000-$9FFFFF
            0, // $A00000-$BFFFFF
            0, // $C00000-$DFFFFF
            3, // $E00000-$FFFFFF
        ];

        // Track memory refresh delay, which stalls the 68000 for roughly 2 out of every 130 CPU cycles
        // when executing from ROM (at least on the standalone Genesis)
        // Clue and Super Airwolf depend on this or they will have graphical glitches, Clue in the
        // main menu and Super Airwolf in the intro
        //
        // TODO this implementation is only approximate, e.g. ROM and RAM refreshes are not aligned
        // and it's possible for the CPU to trigger both independently. A more accurate implementation
        // requires more accurate 68000 timing, in particular tracking the exact cycles on which
        // memory accesses occur
        if vdp_owns_bus {
            // Not sure this is accurate, but it's required for stable images in Direct Color DMA demos
            self.m68k_refresh_counter = 0;
        } else {
            self.m68k_refresh_counter += m68k_cycles;
            if self.m68k_refresh_counter >= REFRESH_INTERVAL {
                if !m68k_wait {
                    let wait_cycles = REGION_WAIT_CYCLES[((m68k_pc >> 21) & 7) as usize];
                    self.m68k_wait_cpu_cycles += wait_cycles;
                }
                self.m68k_refresh_counter %= REFRESH_INTERVAL;
            }
        }

        let mclk_cycles = u64::from(m68k_cycles) * self.m68k_divider.get();
        self.increment_mclk_counters(mclk_cycles, vdp_owns_bus);

        mclk_cycles
    }

    #[inline]
    pub fn increment_mclk_counters(&mut self, mclk_cycles: u64, vdp_owns_bus: bool) {
        self.vdp_owns_bus = vdp_owns_bus;
        self.z80_halt &= vdp_owns_bus;

        self.m68k_mclk_cycles += mclk_cycles;
        if self.z80_halt {
            self.z80_mclk_cycles += mclk_cycles;
        }
    }

    #[inline]
    pub fn record_z80_68k_bus_access(&mut self) {
        // Each time the Z80 accesses the 68K bus, the Z80 is stalled for on average 3 Z80 cycles
        // and the 68K is stalled for on average slightly less than 9.7 68K cycles (based on test ROM)
        self.z80_mclk_cycles += 3 * Z80_DIVIDER;

        // The Z80 should halt if it accesses the 68K bus during a VDP DMA or while the 68K is
        // stalled on a VDP FIFO write
        self.z80_halt |= self.vdp_owns_bus;

        if !self.vdp_owns_bus {
            // Not sure if it's accurate for this to be conditional, but adding this delay after
            // a VDP DMA breaks some effects in Overdrive
            self.m68k_wait_cpu_cycles += 9 + u32::from(self.m68k_wait_counter < 7);
            self.m68k_wait_counter += 1;
            if self.m68k_wait_counter == 10 {
                self.m68k_wait_counter = 0;
            }
        }
    }

    #[inline]
    pub fn record_68k_z80_bus_access(&mut self) {
        // Each time the 68K accesses the Z80 bus, the 68K is stalled for 1 CPU cycle
        // Pac-Man 2: The New Adventures depends on this for its audio code to work correctly
        self.m68k_wait_cpu_cycles += 1;
    }

    #[inline]
    #[must_use]
    pub fn should_tick_z80(&self) -> bool {
        self.z80_mclk_cycles + Z80_DIVIDER <= self.m68k_mclk_cycles
    }

    #[inline]
    pub fn z80_cycle(&mut self) {
        self.z80_mclk_cycles += Z80_DIVIDER;
    }

    #[inline]
    #[must_use]
    pub fn has_ym2612_ticks(&mut self) -> bool {
        self.ym2612_mclk_cycles + YM2612_DIVIDER <= self.z80_mclk_cycles
    }

    #[inline]
    #[must_use]
    pub fn take_ym2612_ticks(&mut self) -> u32 {
        let ticks = (self.z80_mclk_cycles - self.ym2612_mclk_cycles) / YM2612_DIVIDER;
        self.ym2612_mclk_cycles += ticks * YM2612_DIVIDER;
        ticks as u32
    }

    #[inline]
    #[must_use]
    pub fn should_tick_psg(&self) -> bool {
        self.psg_mclk_cycles + PSG_DIVIDER <= self.m68k_mclk_cycles
    }

    #[inline]
    pub fn psg_cycle(&mut self) {
        self.psg_mclk_cycles += PSG_DIVIDER;
    }

    #[inline]
    pub fn maybe_sync_and_drain_ym2612(
        &mut self,
        vdp_tick_effect: VdpTickEffect,
        ym2612: &mut Ym2612,
        mut output: impl FnMut((f64, f64)),
    ) {
        if vdp_tick_effect != VdpTickEffect::FrameComplete
            && self.last_ym2612_drain_mclk + MAX_YM2612_LAG_MCLK > self.z80_mclk_cycles
        {
            return;
        }

        if self.has_ym2612_ticks() {
            let ticks = self.take_ym2612_ticks();
            ym2612.tick(ticks);
        }

        for sample in ym2612.drain_output_samples() {
            output(sample);
        }

        self.last_ym2612_drain_mclk = self.z80_mclk_cycles;
    }
}

pub const GENESIS_REFRESH_INTERVAL: u32 = 128;

pub type GenesisCycleCounters = CycleCounters<GENESIS_REFRESH_INTERVAL>;
