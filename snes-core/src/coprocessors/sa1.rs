mod bus;
mod mmc;
mod registers;
mod timer;

use crate::coprocessors::sa1::bus::Sa1Bus;
use crate::coprocessors::sa1::mmc::Sa1Mmc;
use crate::coprocessors::sa1::registers::Sa1Registers;
use crate::coprocessors::sa1::timer::Sa1Timer;
use crate::memory::cartridge::Rom;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::PartialClone;
use std::mem;
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
    mmc: Sa1Mmc,
    registers: Sa1Registers,
    timer: Sa1Timer,
}

impl Sa1 {
    pub fn new(rom: Rom, sram: Box<[u8]>, timing_mode: TimingMode) -> Self {
        Self {
            rom,
            iram: vec![0; IRAM_LEN].into_boxed_slice().try_into().unwrap(),
            bwram: sram,
            cpu: Wdc65816::new(),
            mmc: Sa1Mmc::new(),
            registers: Sa1Registers::new(),
            timer: Sa1Timer::new(timing_mode),
        }
    }

    pub fn take_rom(&mut self) -> Vec<u8> {
        mem::take(&mut self.rom.0).into_vec()
    }

    pub fn set_rom(&mut self, rom: Vec<u8>) {
        self.rom.0 = rom.into_boxed_slice();
    }

    pub fn sram(&self) -> Option<&[u8]> {
        (!self.bwram.is_empty()).then_some(self.bwram.as_ref())
    }

    pub fn tick(&mut self, master_cycles_elapsed: u64) {
        assert_eq!(master_cycles_elapsed % 2, 0);

        let mut bus = new_sa1_bus!(self);
        for _ in 0..master_cycles_elapsed / 2 {
            if !bus.registers.cpu_halted() {
                self.cpu.tick(&mut bus);
            }

            bus.registers.tick(bus.mmc, bus.rom, bus.iram, bus.bwram);
            bus.timer.tick();
        }
    }

    pub fn snes_irq(&self) -> bool {
        (self.registers.snes_irq_from_sa1_enabled && self.registers.snes_irq_from_sa1)
            || (self.registers.snes_irq_from_dma_enabled && self.registers.character_conversion_irq)
    }

    pub fn reset(&mut self) {
        self.registers.reset(&mut self.timer, &mut self.mmc);
    }

    pub fn notify_dma_start(&mut self, source_address: u32) {
        self.registers.notify_snes_dma_start(source_address);
    }

    pub fn notify_dma_end(&mut self) {
        self.registers.notify_snes_dma_end();
    }
}
