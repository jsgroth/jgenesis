//! 32X core code

use crate::api::Sega32XEmulatorConfig;
use crate::audio::PwmResampler;
use crate::bootrom;
use crate::bootrom::M68kVectors;
use crate::bus::{OtherCpu, Sh2Bus, WhichCpu};
use crate::pwm::PwmChip;
use crate::registers::SystemRegisters;
use crate::vdp::Vdp;
use bincode::{Decode, Encode};
use genesis_config::GenesisRegion;
use genesis_core::cartridge::Cartridge;
use genesis_core::timing;
use jgenesis_common::boxedarray::BoxedWordArray;
use jgenesis_common::debug::DebugMemoryView;
use jgenesis_common::frontend::TimingMode;
use jgenesis_proc_macros::PartialClone;
use sh2_emu::Sh2;
use std::cmp;
use std::num::NonZeroU64;

const M68K_DIVIDER: u64 = timing::NATIVE_M68K_DIVIDER;
const SH2_MULTIPLIER: u64 = crate::SH2_CLOCK_MULTIPLIER;

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
    pub log_write_ranges: Vec<(u32, u32)>,
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32X {
    sh2_master: Sh2,
    sh2_slave: Sh2,
    mclk_counter: u64,
    global_cycles: u64,
    master_cycles: u64,
    slave_cycles: u64,
    sh2_clock_multiplier: Option<NonZeroU64>,
    #[partial_clone(partial)]
    pub s32x_bus: Sega32XBus,
    pub m68k_vectors: Box<M68kVectors>,
    pub region: GenesisRegion,
    pub timing_mode: TimingMode,
}

impl Sega32X {
    pub fn new(rom: Vec<u8>, initial_ram: Option<Vec<u8>>, config: &Sega32XEmulatorConfig) -> Self {
        let cartridge = Cartridge::from_rom(rom, initial_ram, config.genesis.forced_region);

        let region = cartridge.region();
        let timing_mode = config.genesis.forced_timing_mode.unwrap_or(match region {
            GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
            GenesisRegion::Europe => TimingMode::Pal,
        });

        Self {
            sh2_master: Sh2::new("Master".into()),
            sh2_slave: Sh2::new("Slave".into()),
            mclk_counter: 0,
            global_cycles: 0,
            master_cycles: 0,
            slave_cycles: 0,
            sh2_clock_multiplier: none_if_default_multiplier(config.sh2_clock_multiplier),
            s32x_bus: Sega32XBus {
                cartridge,
                vdp: Vdp::new(timing_mode, config),
                pwm: PwmChip::new(timing_mode),
                registers: SystemRegisters::new(),
                sdram: BoxedWordArray::new(),
                serial: SerialInterface::default(),
                log_write_ranges: config.log_write_address_ranges.clone(),
            },
            m68k_vectors: Box::new(bootrom::new_m68k_vectors()),
            region,
            timing_mode,
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

            self.mclk_counter += mclk_cycles;
            let (elapsed_sh2_cycles, elapsed_pwm_cycles) = match self.sh2_clock_multiplier {
                Some(multiplier) => {
                    let multiplier = multiplier.get();
                    let elapsed_sh2_cycles = self.mclk_counter / M68K_DIVIDER * multiplier;
                    let elapsed_pwm_cycles = elapsed_sh2_cycles / multiplier * SH2_MULTIPLIER;
                    self.mclk_counter -= elapsed_sh2_cycles * M68K_DIVIDER / multiplier;

                    (elapsed_sh2_cycles, elapsed_pwm_cycles)
                }
                None => {
                    let elapsed_sh2_cycles = self.mclk_counter / M68K_DIVIDER * SH2_MULTIPLIER;
                    self.mclk_counter -= elapsed_sh2_cycles * M68K_DIVIDER / SH2_MULTIPLIER;

                    (elapsed_sh2_cycles, elapsed_sh2_cycles)
                }
            };

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
            self.sh2_master.tick_peripherals(elapsed_pwm_cycles, &mut peripherals_bus);

            peripherals_bus.which = WhichCpu::Slave;
            self.sh2_slave.tick_peripherals(elapsed_pwm_cycles, &mut peripherals_bus);

            self.s32x_bus.vdp.tick(mclk_cycles, &mut self.s32x_bus.registers, genesis_vdp);

            self.s32x_bus.pwm.tick(elapsed_pwm_cycles, &mut self.s32x_bus.registers, pwm_resampler);
        }
    }

    pub fn take_rom_from(&mut self, other: &mut Self) {
        self.s32x_bus.cartridge.take_rom_from(&mut other.s32x_bus.cartridge);
    }

    pub fn reload_config(&mut self, config: &Sega32XEmulatorConfig) {
        self.sh2_clock_multiplier = none_if_default_multiplier(config.sh2_clock_multiplier);
        self.s32x_bus.vdp.reload_config(config);
        self.s32x_bus.log_write_ranges.clone_from(&config.log_write_address_ranges);
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

    pub fn debug_master_sh2_cache(&mut self) -> impl DebugMemoryView {
        self.sh2_master.debug_cache_view()
    }

    pub fn debug_slave_sh2_cache(&mut self) -> impl DebugMemoryView {
        self.sh2_slave.debug_cache_view()
    }
}

fn none_if_default_multiplier(multiplier: NonZeroU64) -> Option<NonZeroU64> {
    match multiplier.get() {
        SH2_MULTIPLIER => None,
        _ => Some(multiplier),
    }
}
