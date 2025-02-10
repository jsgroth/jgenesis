//! SA-1 coprocessor, a second 65C816 CPU clocked at 10.74 MHz
//!
//! Used by ~35 games including Kirby Super Star, Kirby's Dream Land 3, Super Mario RPG

mod bus;
mod mmc;
mod registers;
mod timer;

use crate::common;
use crate::common::{Rom, impl_take_set_rom};
use crate::sa1::bus::Sa1Bus;
use crate::sa1::mmc::Sa1Mmc;
use crate::sa1::registers::{DmaState, Sa1Registers};
use crate::sa1::timer::Sa1Timer;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::PartialClone;
use std::cmp;
use wdc65816_emu::core::Wdc65816;

const IRAM_LEN: usize = 2 * 1024;

type Iram = [u8; IRAM_LEN];

macro_rules! new_sa1_bus {
    ($self:expr) => {
        Sa1Bus {
            rom: &$self.rom,
            iram: &mut $self.iram,
            bwram: &mut $self.bwram,
            mmc: &mut $self.mmc,
            registers: &mut $self.registers,
            timer: &mut $self.timer,
            bwram_wait_cycles: 0,
        }
    };
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Sa1 {
    #[partial_clone(default)]
    rom: Rom,
    iram: Box<Iram>,
    bwram: Box<[u8]>,
    cpu: Wdc65816,
    bwram_wait_cycles: u64,
    mmc: Sa1Mmc,
    registers: Sa1Registers,
    timer: Sa1Timer,
}

impl Sa1 {
    #[allow(clippy::missing_panics_doc)]
    #[must_use]
    pub fn new(rom: Box<[u8]>, sram: Box<[u8]>, timing_mode: TimingMode) -> Self {
        Self {
            rom: Rom(rom),
            iram: vec![0; IRAM_LEN].into_boxed_slice().try_into().unwrap(),
            bwram: sram,
            cpu: Wdc65816::new(),
            bwram_wait_cycles: 0,
            mmc: Sa1Mmc::new(),
            registers: Sa1Registers::new(),
            timer: Sa1Timer::new(timing_mode),
        }
    }

    impl_take_set_rom!(rom);

    #[inline]
    #[must_use]
    pub fn has_battery(&self) -> bool {
        // Most SA-1 games have battery backup, but Dragon Ball Z: Hyper Dimension does not
        // This is indicated by a chipset byte of $34 instead of $32 or $35
        let chipset_byte = self.rom[common::LOROM_CHIPSET_BYTE_ADDRESS];
        !self.bwram.is_empty() && (chipset_byte == 0x32 || chipset_byte == 0x35)
    }

    #[inline]
    #[must_use]
    pub fn sram(&self) -> Option<&[u8]> {
        (!self.bwram.is_empty()).then_some(self.bwram.as_ref())
    }

    #[inline]
    /// # Panics
    ///
    /// This method will panic if `master_cycles_elapsed` is not a multiple of 2.
    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        assert_eq!(master_cycles_elapsed % 2, 0);

        // 10.74 MHz clock, SNES mclk / 2
        let sa1_cycles = master_cycles_elapsed / 2;

        let spent_wait_cycles = cmp::min(sa1_cycles, self.bwram_wait_cycles);
        let cpu_cycles = sa1_cycles - spent_wait_cycles;
        self.bwram_wait_cycles -= spent_wait_cycles;

        if !self.registers.cpu_halted() {
            let mut bus = new_sa1_bus!(self);
            for _ in 0..cpu_cycles {
                self.cpu.tick(&mut bus);
            }
            self.bwram_wait_cycles += bus.bwram_wait_cycles;
        }

        if self.registers.dma_state != DmaState::Idle {
            for _ in 0..sa1_cycles {
                self.registers.tick_dma(&self.mmc, &self.rom, &mut self.iram, &mut self.bwram);
            }
        }

        for _ in 0..sa1_cycles {
            self.timer.tick();
        }
    }

    #[inline]
    #[must_use]
    pub fn snes_irq(&self) -> bool {
        (self.registers.snes_irq_from_sa1_enabled && self.registers.snes_irq_from_sa1)
            || (self.registers.snes_irq_from_dma_enabled && self.registers.character_conversion_irq)
    }

    pub fn reset(&mut self) {
        self.registers.reset(&mut self.timer, &mut self.mmc);
    }

    #[inline]
    pub fn notify_dma_start(&mut self, source_address: u32) {
        self.registers.notify_snes_dma_start(source_address);
    }

    #[inline]
    pub fn notify_dma_end(&mut self) {
        self.registers.notify_snes_dma_end();
    }
}
