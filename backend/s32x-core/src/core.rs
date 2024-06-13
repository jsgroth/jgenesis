//! 32X core code

use crate::api;
use crate::bus::{Sh2Bus, WhichCpu};
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::{FakeDecode, FakeEncode, PartialClone};
use sh2_emu::Sh2;
use std::mem;
use std::ops::Deref;

const SH2_MASTER_BOOT_ROM: &[u8; 2048] = include_bytes!("sh2_master_boot_rom.bin");
const SH2_SLAVE_BOOT_ROM: &[u8; 1024] = include_bytes!("sh2_slave_boot_rom.bin");

const SDRAM_LEN_WORDS: usize = 256 * 1024 / 2;

pub type Sdram = [u16; SDRAM_LEN_WORDS];

#[derive(Debug, Clone, Default, FakeEncode, FakeDecode)]
pub struct Rom(Box<[u8]>);

impl Deref for Rom {
    type Target = Box<[u8]>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Rom {
    pub fn get_u16(&self, address: u32) -> u16 {
        let address = address as usize;
        if address + 1 < self.0.len() {
            u16::from_be_bytes(self.0[address..address + 2].try_into().unwrap())
        } else {
            0xFFFF
        }
    }

    pub fn get_u32(&self, address: u32) -> u32 {
        let address = address as usize;
        if address + 3 < self.0.len() {
            u32::from_be_bytes(self.0[address..address + 4].try_into().unwrap())
        } else {
            0xFFFFFFFF
        }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32X {
    sh2_master: Sh2,
    sh2_slave: Sh2,
    sh2_cycles: u64,
    #[partial_clone(default)]
    pub rom: Rom,
    pub vdp: Vdp,
    pub registers: SystemRegisters,
    pub sdram: Box<Sdram>,
}

impl Sega32X {
    pub fn new(rom: Box<[u8]>, timing_mode: TimingMode) -> Self {
        Self {
            sh2_master: Sh2::new("Master".into()),
            sh2_slave: Sh2::new("Slave".into()),
            sh2_cycles: 0,
            rom: Rom(rom),
            vdp: Vdp::new(timing_mode),
            registers: SystemRegisters::new(),
            sdram: vec![0; SDRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
        }
    }

    pub fn tick(&mut self, m68k_cycles: u64) {
        self.vdp.tick(api::M68K_DIVIDER * m68k_cycles);

        if !self.registers.adapter_enabled {
            return;
        }

        // SH-2 clock speed is exactly 3x the 68000 clock speed
        self.sh2_cycles += 3 * m68k_cycles;

        // TODO actual timing
        let sh2_ticks = self.sh2_cycles / 2;
        self.sh2_cycles %= 2;

        let mut master_bus = Sh2Bus {
            boot_rom: SH2_MASTER_BOOT_ROM,
            boot_rom_mask: SH2_MASTER_BOOT_ROM.len() - 1,
            which: WhichCpu::Master,
            rom: &self.rom,
            vdp: &mut self.vdp,
            registers: &mut self.registers,
            sdram: &mut self.sdram,
        };
        for _ in 0..sh2_ticks {
            self.sh2_master.tick(&mut master_bus);
        }

        let mut slave_bus = Sh2Bus {
            boot_rom: SH2_SLAVE_BOOT_ROM,
            boot_rom_mask: SH2_SLAVE_BOOT_ROM.len() - 1,
            which: WhichCpu::Slave,
            rom: &self.rom,
            vdp: &mut self.vdp,
            registers: &mut self.registers,
            sdram: &mut self.sdram,
        };
        for _ in 0..sh2_ticks {
            self.sh2_slave.tick(&mut slave_bus);
        }
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.rom.0 = mem::take(&mut other.rom.0);
    }
}
