//! 32X core code

use crate::api;
use crate::api::Sega32XEmulatorConfig;
use crate::audio::PwmResampler;
use crate::bus::{Sh2Bus, WhichCpu};
use crate::cartridge::Cartridge;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::PartialClone;
use sh2_emu::Sh2;
use std::mem;

pub type M68kVectors = [u8; 256];

const M68K_VECTORS: &M68kVectors = include_bytes!("m68k_vectors.bin");
const SH2_MASTER_BOOT_ROM: &[u8; 2048] = include_bytes!("sh2_master_boot_rom.bin");
const SH2_SLAVE_BOOT_ROM: &[u8; 1024] = include_bytes!("sh2_slave_boot_rom.bin");

const SDRAM_LEN_WORDS: usize = 256 * 1024 / 2;

pub type Sdram = [u16; SDRAM_LEN_WORDS];

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32X {
    sh2_master: Sh2,
    sh2_slave: Sh2,
    sh2_cycles: u64,
    #[partial_clone(partial)]
    pub cartridge: Cartridge,
    pub vdp: Vdp,
    pub pwm: PwmChip,
    pub registers: SystemRegisters,
    pub m68k_vectors: Box<M68kVectors>,
    pub sdram: Box<Sdram>,
}

impl Sega32X {
    pub fn new(
        rom: Box<[u8]>,
        initial_ram: Option<Vec<u8>>,
        timing_mode: TimingMode,
        config: Sega32XEmulatorConfig,
    ) -> Self {
        let cartridge = Cartridge::new(rom, initial_ram);

        Self {
            sh2_master: Sh2::new("Master".into()),
            sh2_slave: Sh2::new("Slave".into()),
            sh2_cycles: 0,
            cartridge,
            vdp: Vdp::new(timing_mode, config.video_out),
            pwm: PwmChip::new(timing_mode),
            registers: SystemRegisters::new(),
            m68k_vectors: M68K_VECTORS.to_vec().into_boxed_slice().try_into().unwrap(),
            sdram: vec![0; SDRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
        }
    }

    pub fn tick(&mut self, m68k_cycles: u64, pwm_resampler: &mut PwmResampler) {
        self.vdp.tick(api::M68K_DIVIDER * m68k_cycles, &mut self.registers);

        if !self.registers.adapter_enabled {
            return;
        }

        // SH-2 clock speed is exactly 3x the 68000 clock speed
        let elapsed_sh2_cycles = 3 * m68k_cycles;
        self.sh2_cycles += elapsed_sh2_cycles;

        // TODO actual timing instead of hardcoded 1.5 cycles per instruction
        let sh2_ticks = self.sh2_cycles * 2 / 3;
        self.sh2_cycles -= sh2_ticks * 3 / 2;

        let mut bus = Sh2Bus {
            boot_rom: SH2_MASTER_BOOT_ROM,
            boot_rom_mask: SH2_MASTER_BOOT_ROM.len() - 1,
            which: WhichCpu::Master,
            cartridge: &self.cartridge,
            vdp: &mut self.vdp,
            pwm: &mut self.pwm,
            registers: &mut self.registers,
            sdram: &mut self.sdram,
        };
        self.sh2_master.tick(sh2_ticks, &mut bus);

        self.sh2_master.tick_timers(elapsed_sh2_cycles);

        bus.boot_rom = SH2_SLAVE_BOOT_ROM;
        bus.boot_rom_mask = SH2_SLAVE_BOOT_ROM.len() - 1;
        bus.which = WhichCpu::Slave;
        self.sh2_slave.tick(sh2_ticks, &mut bus);

        self.sh2_slave.tick_timers(elapsed_sh2_cycles);

        self.pwm.tick(elapsed_sh2_cycles, &mut self.registers, pwm_resampler);
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.cartridge.rom.0 = mem::take(&mut other.cartridge.rom.0);
    }

    pub fn reload_config(&mut self, config: Sega32XEmulatorConfig) {
        self.vdp.update_video_out(config.video_out);
    }
}