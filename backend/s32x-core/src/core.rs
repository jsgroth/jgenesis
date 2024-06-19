//! 32X core code

use crate::api::Sega32XEmulatorConfig;
use crate::audio::PwmResampler;
use crate::bootrom::M68kVectors;
use crate::bus::{Sh2Bus, WhichCpu};
use crate::cartridge::Cartridge;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use crate::{api, bootrom};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::PartialClone;
use sh2_emu::Sh2;
use std::mem;

// Only execute SH-2 instructions in batches of at least 10 for slightly better performance
const SH2_EXECUTION_SLICE_LEN: u64 = 10;

const SDRAM_LEN_WORDS: usize = 256 * 1024 / 2;

pub type Sdram = [u16; SDRAM_LEN_WORDS];

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct SerialInterface {
    pub master_to_slave: Option<u8>,
    pub slave_to_master: Option<u8>,
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32X {
    sh2_master: Sh2,
    sh2_slave: Sh2,
    sh2_cycles: u64,
    sh2_ticks: u64,
    #[partial_clone(partial)]
    pub cartridge: Cartridge,
    pub vdp: Vdp,
    pub pwm: PwmChip,
    pub registers: SystemRegisters,
    pub m68k_vectors: Box<M68kVectors>,
    pub sdram: Box<Sdram>,
    pub serial: SerialInterface,
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
            sh2_ticks: 0,
            cartridge,
            vdp: Vdp::new(timing_mode, config.video_out),
            pwm: PwmChip::new(timing_mode),
            registers: SystemRegisters::new(),
            m68k_vectors: bootrom::M68K_VECTORS.to_vec().into_boxed_slice().try_into().unwrap(),
            sdram: vec![0; SDRAM_LEN_WORDS].into_boxed_slice().try_into().unwrap(),
            serial: SerialInterface::default(),
        }
    }

    pub fn tick(&mut self, m68k_cycles: u64, pwm_resampler: &mut PwmResampler) {
        self.vdp.tick(api::M68K_DIVIDER * m68k_cycles, &mut self.registers);

        // SH-2 clock speed is exactly 3x the 68000 clock speed
        let elapsed_sh2_cycles = 3 * m68k_cycles;
        self.sh2_cycles += elapsed_sh2_cycles;

        // TODO actual timing instead of hardcoded 1.6 cycles per instruction
        let new_sh2_ticks = self.sh2_cycles * 5 / 8;
        self.sh2_cycles -= new_sh2_ticks * 8 / 5;
        self.sh2_ticks += new_sh2_ticks;

        let mut bus = Sh2Bus {
            which: WhichCpu::Master,
            cartridge: &mut self.cartridge,
            vdp: &mut self.vdp,
            pwm: &mut self.pwm,
            registers: &mut self.registers,
            sdram: &mut self.sdram,
            serial: &mut self.serial,
        };

        // Run the two CPUs in slices of 10 instructions rather than running one for N instructions
        // then the other for N instructions. Brutal Unleashed: Above the Claw requires fairly
        // close synchronization between the two SH-2s to avoid freezing, and this is a lot simpler
        // than syncing on communication port accesses
        while self.sh2_ticks >= SH2_EXECUTION_SLICE_LEN {
            self.sh2_ticks -= SH2_EXECUTION_SLICE_LEN;

            // Running the slave SH-2 before the master SH-2 fixes some of the Knuckles Chaotix
            // prototype cartridges, which can freeze at boot otherwise.
            // The 68000 writes to a communication port after it's done initializing the 32X VDP,
            // and both SH-2s need to see that write before the master SH-2 writes a different value
            // to the port (which it does almost immediately)
            bus.which = WhichCpu::Slave;
            self.sh2_slave.execute(SH2_EXECUTION_SLICE_LEN, &mut bus);

            bus.which = WhichCpu::Master;
            self.sh2_master.execute(SH2_EXECUTION_SLICE_LEN, &mut bus);
        }

        bus.which = WhichCpu::Master;
        self.sh2_master.tick_peripherals(elapsed_sh2_cycles, &mut bus);

        bus.which = WhichCpu::Slave;
        self.sh2_slave.tick_peripherals(elapsed_sh2_cycles, &mut bus);

        self.pwm.tick(elapsed_sh2_cycles, &mut self.registers, pwm_resampler);
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.cartridge.rom.0 = mem::take(&mut other.cartridge.rom.0);
    }

    pub fn reload_config(&mut self, config: Sega32XEmulatorConfig) {
        self.vdp.update_video_out(config.video_out);
    }
}
