//! 32X core code

use crate::api::Sega32XEmulatorConfig;
use crate::audio::PwmResampler;
use crate::bootrom;
use crate::bootrom::M68kVectors;
use crate::bus::{OtherCpu, Sh2Bus, WhichCpu};
use crate::cartridge::Cartridge;
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use bincode::{Decode, Encode};
use genesis_config::GenesisRegion;
use genesis_core::timing;
use jgenesis_common::boxedarray::BoxedWordArray;
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::PartialClone;
use sh2_emu::Sh2;
use std::{cmp, mem};

const M68K_DIVIDER: u64 = timing::NATIVE_M68K_DIVIDER;
const SH2_MULTIPLIER: u64 = 3;

// Prefer to execute SH-2 instructions in longer chunks when possible for better performance
pub const SH2_EXECUTION_SLICE_LEN: u64 = 50;

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
            m68k_vectors: Box::new(*bootrom::M68K_VECTORS),
            region,
        }
    }

    pub fn tick(
        &mut self,
        mut total_mclk_cycles: u64,
        pwm_resampler: &mut PwmResampler,
        genesis_vdp: &genesis_core::vdp::Vdp,
    ) {
        while total_mclk_cycles > 0 {
            let h_interrupt_enabled = self.s32x_bus.registers.either_h_interrupt_enabled();
            let mclk_till_next_vdp_event =
                self.s32x_bus.vdp.mclk_cycles_until_next_event(h_interrupt_enabled);
            debug_assert_ne!(mclk_till_next_vdp_event, 0);

            let mclk_cycles = cmp::min(mclk_till_next_vdp_event, total_mclk_cycles);
            total_mclk_cycles -= mclk_cycles;

            // SH-2 clock speed is exactly 3x the 68000 clock speed
            self.mclk_counter += mclk_cycles;
            let elapsed_sh2_cycles = self.mclk_counter / M68K_DIVIDER * SH2_MULTIPLIER;
            self.mclk_counter -= elapsed_sh2_cycles * M68K_DIVIDER / SH2_MULTIPLIER;
            self.global_cycles += elapsed_sh2_cycles;

            let mut slave_bus = Sh2Bus {
                s32x_bus: &mut self.s32x_bus,
                which: WhichCpu::Slave,
                cycle_counter: self.slave_cycles,
                cycle_limit: self.global_cycles,
                other_sh2: Some(OtherCpu {
                    cpu: &mut self.sh2_master,
                    cycle_counter: &mut self.master_cycles,
                }),
            };
            while slave_bus.cycle_counter < self.global_cycles {
                self.sh2_slave.execute(SH2_EXECUTION_SLICE_LEN, &mut slave_bus);
            }
            self.slave_cycles = slave_bus.cycle_counter;

            let mut master_bus = Sh2Bus {
                s32x_bus: &mut self.s32x_bus,
                which: WhichCpu::Master,
                cycle_counter: self.master_cycles,
                cycle_limit: self.global_cycles,
                other_sh2: Some(OtherCpu {
                    cpu: &mut self.sh2_slave,
                    cycle_counter: &mut self.slave_cycles,
                }),
            };
            while master_bus.cycle_counter < self.global_cycles {
                self.sh2_master.execute(SH2_EXECUTION_SLICE_LEN, &mut master_bus);
            }
            self.master_cycles = master_bus.cycle_counter;

            let mut peripherals_bus = Sh2Bus {
                s32x_bus: &mut self.s32x_bus,
                which: WhichCpu::Master,
                cycle_counter: 0,
                cycle_limit: 0,
                other_sh2: None,
            };
            self.sh2_master.tick_peripherals(elapsed_sh2_cycles, &mut peripherals_bus);

            peripherals_bus.which = WhichCpu::Slave;
            self.sh2_slave.tick_peripherals(elapsed_sh2_cycles, &mut peripherals_bus);

            self.s32x_bus.vdp.tick(mclk_cycles, &mut self.s32x_bus.registers, genesis_vdp);

            self.s32x_bus.pwm.tick(elapsed_sh2_cycles, &mut self.s32x_bus.registers, pwm_resampler);
        }
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
