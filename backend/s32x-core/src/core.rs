//! 32X core code

use crate::api::Sega32XEmulatorConfig;
use crate::audio::PwmResampler;
use crate::bootrom;
use crate::bootrom::M68kVectors;
use crate::bus::{Sh2Bus, WhichCpu};
use crate::cartridge::Cartridge;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use bincode::{Decode, Encode};
use genesis_core::{GenesisRegion, timing};
use jgenesis_common::boxedarray::BoxedWordArray;
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::PartialClone;
use sh2_emu::Sh2;
use std::mem;

const M68K_DIVIDER: u64 = timing::NATIVE_M68K_DIVIDER;
const SH2_MULTIPLIER: u64 = 3;

// Only execute SH-2 instructions in batches of at least 10 for slightly better performance
const SH2_EXECUTION_SLICE_LEN: u64 = 10;

const SDRAM_LEN_WORDS: usize = 256 * 1024 / 2;

#[derive(Debug, Clone, Default, Encode, Decode)]
pub struct SerialInterface {
    pub master_to_slave: Option<u8>,
    pub slave_to_master: Option<u8>,
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32XBus {
    #[partial_clone(partial)]
    pub cartridge: Cartridge,
    pub vdp: Vdp,
    pub pwm: PwmChip,
    pub registers: SystemRegisters,
    pub sdram: BoxedWordArray<SDRAM_LEN_WORDS>,
    pub serial: SerialInterface,
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32X {
    sh2_master: Sh2,
    sh2_slave: Sh2,
    mclk_counter: u64,
    global_cycles: u64,
    master_cycles: u64,
    slave_cycles: u64,
    #[partial_clone(partial)]
    pub s32x_bus: Sega32XBus,
    pub m68k_vectors: Box<M68kVectors>,
    pub region: GenesisRegion,
}

impl Sega32X {
    pub fn new(
        rom: Box<[u8]>,
        initial_ram: Option<Vec<u8>>,
        region: GenesisRegion,
        timing_mode: TimingMode,
        config: Sega32XEmulatorConfig,
    ) -> Self {
        let cartridge = Cartridge::new(rom, initial_ram);

        Self {
            sh2_master: Sh2::new("Master".into()),
            sh2_slave: Sh2::new("Slave".into()),
            mclk_counter: 0,
            global_cycles: 0,
            master_cycles: 0,
            slave_cycles: 0,
            s32x_bus: Sega32XBus {
                cartridge,
                vdp: Vdp::new(timing_mode, config.video_out),
                pwm: PwmChip::new(timing_mode),
                registers: SystemRegisters::new(),
                sdram: BoxedWordArray::new(),
                serial: SerialInterface::default(),
            },
            m68k_vectors: bootrom::M68K_VECTORS.to_vec().into_boxed_slice().try_into().unwrap(),
            region,
        }
    }

    pub fn tick(&mut self, mclk_cycles: u64, pwm_resampler: &mut PwmResampler) {
        self.s32x_bus.vdp.tick(mclk_cycles, &mut self.s32x_bus.registers);

        // SH-2 clock speed is exactly 3x the 68000 clock speed
        self.mclk_counter += mclk_cycles;
        let elapsed_sh2_cycles = self.mclk_counter * SH2_MULTIPLIER / M68K_DIVIDER;
        self.mclk_counter -= elapsed_sh2_cycles * M68K_DIVIDER / SH2_MULTIPLIER;
        self.global_cycles += elapsed_sh2_cycles;

        // Brutal Unleashed: Above the Claw requires fairly close synchronization to prevent
        // the game from freezing due to the master SH-2 missing a communication port write from
        // the slave SH-2. After the slave SH-2 sees a specific value from the master SH-2, it
        // writes to the communication port twice in quick succession, and the master SH-2 must
        // read the first value before it's overwritten
        while self.master_cycles < self.global_cycles || self.slave_cycles < self.global_cycles {
            // Running the slave SH-2 before the master SH-2 fixes some of the Knuckles Chaotix
            // prototype cartridges, which can freeze at boot otherwise.
            // The 68000 writes to a communication port after it's done initializing the 32X VDP,
            // and both SH-2s need to see that write before the master SH-2 writes a different value
            // to the port (which it does almost immediately)
            if self.slave_cycles < self.global_cycles {
                while self.slave_cycles <= self.master_cycles {
                    let mut bus = Sh2Bus {
                        s32x_bus: &mut self.s32x_bus,
                        which: WhichCpu::Slave,
                        cycle_counter: self.slave_cycles,
                    };
                    self.sh2_slave.execute(SH2_EXECUTION_SLICE_LEN, &mut bus);
                    self.slave_cycles = bus.cycle_counter + SH2_EXECUTION_SLICE_LEN;
                }
            }

            if self.master_cycles < self.global_cycles {
                while self.master_cycles <= self.slave_cycles {
                    let mut bus = Sh2Bus {
                        s32x_bus: &mut self.s32x_bus,
                        which: WhichCpu::Master,
                        cycle_counter: self.master_cycles,
                    };
                    self.sh2_master.execute(SH2_EXECUTION_SLICE_LEN, &mut bus);
                    self.master_cycles = bus.cycle_counter + SH2_EXECUTION_SLICE_LEN;
                }
            }
        }

        self.sh2_master.tick_peripherals(elapsed_sh2_cycles, &mut Sh2Bus {
            s32x_bus: &mut self.s32x_bus,
            which: WhichCpu::Master,
            cycle_counter: 0,
        });

        self.sh2_slave.tick_peripherals(elapsed_sh2_cycles, &mut Sh2Bus {
            s32x_bus: &mut self.s32x_bus,
            which: WhichCpu::Slave,
            cycle_counter: 0,
        });

        self.s32x_bus.pwm.tick(elapsed_sh2_cycles, &mut self.s32x_bus.registers, pwm_resampler);
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.s32x_bus.cartridge.rom.0 = mem::take(&mut other.s32x_bus.cartridge.rom.0);
    }

    pub fn reload_config(&mut self, config: Sega32XEmulatorConfig) {
        self.s32x_bus.vdp.update_video_out(config.video_out);
    }

    pub fn reset(&mut self) {
        self.s32x_bus.registers.reset();
    }

    pub fn vdp(&mut self) -> &mut Vdp {
        &mut self.s32x_bus.vdp
    }

    pub fn cartridge(&self) -> &Cartridge {
        &self.s32x_bus.cartridge
    }

    pub fn cartridge_mut(&mut self) -> &mut Cartridge {
        &mut self.s32x_bus.cartridge
    }
}
