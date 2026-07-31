//! 32X public interface and main loop

pub mod debug;

use crate::api::debug::Sega32XDebugger;
use crate::bootrom::M68kVectors;
use crate::bus::debug::DebugSh2Bus;
use crate::bus::{Sega32XBus, Sh2Bus};
use crate::registers::Access;
use crate::{GenesisVdp, WhichCpu, bootrom};
use bincode::{Decode, Encode};
use genesis_components::GenesisEmulatorConfigExt;
use genesis_components::cartridge::Cartridge;
use genesis_components::vdp::BorderSize;
use genesis_config::GenesisEmulatorConfig;
use jgenesis_common::frontend::{FrameSize, Renderer, TimingMode};
use jgenesis_common::num::{GetBit, U16Ext};
use jgenesis_proc_macros::PartialClone;
use sh2_emu::Sh2;
use std::cmp;
use std::fmt::Debug;
use std::num::NonZeroU64;

const M68K_DIVIDER: u64 = genesis_config::NATIVE_M68K_DIVIDER;
const SH2_MULTIPLIER: u64 = crate::SH2_CLOCK_MULTIPLIER;

// Prefer to execute SH-2 instructions in longer chunks when possible for better performance
pub(crate) const SH2_EXECUTION_SLICE_LEN: u64 = 50;

pub trait Sega32XAudioOutput {
    fn collect_pwm(&mut self, sample: (f64, f64));

    fn update_pwm_source_frequency(&mut self, frequency: f64);
}

#[derive(Debug, Clone, Copy)]
pub struct GenesisVdpInfo {
    pub scanline: u16,
    pub scanline_mclk: u64,
    pub frame_size: FrameSize,
    pub border_size: BorderSize,
    pub scanlines_in_current_frame: u16,
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct Sega32X {
    sh2_master: Sh2,
    sh2_slave: Sh2,
    mclk_counter: u64,
    global_cycles: u64,
    master_cycles: u64,
    slave_cycles: u64,
    sh2_clock_multiplier: Option<NonZeroU64>,
    #[partial_clone(partial)]
    bus: Sega32XBus,
    m68k_vectors: Box<M68kVectors>,
    timing_mode: TimingMode,
    config: GenesisEmulatorConfig,
}

impl Sega32X {
    #[must_use]
    pub fn new(
        timing_mode: TimingMode,
        cartridge: Option<Cartridge>,
        config: &GenesisEmulatorConfig,
    ) -> Self {
        Self {
            sh2_master: Sh2::new("Master".into()),
            sh2_slave: Sh2::new("Slave".into()),
            mclk_counter: 0,
            global_cycles: 0,
            master_cycles: 0,
            slave_cycles: 0,
            sh2_clock_multiplier: none_if_default_multiplier(config.sega_32x.sh2_clock_multiplier),
            bus: Sega32XBus::new(timing_mode, cartridge, &config.sega_32x),
            m68k_vectors: Box::new(bootrom::new_m68k_vectors()),
            timing_mode,
            config: config.clone(),
        }
    }

    #[inline]
    pub fn tick<const DEBUG: bool>(
        &mut self,
        mut total_mclk_cycles: u64,
        vdp_info: GenesisVdpInfo,
        audio_output: &mut impl Sega32XAudioOutput,
        debugger: &mut impl Sega32XDebugger,
    ) {
        while total_mclk_cycles > 0 {
            // If an SH-2 is stalled on a cartridge ROM access, it unstalls when RV=0
            self.bus.stalled_on_cartridge_access &= self.bus.registers.dma.rom_to_vram;

            let h_interrupt_enabled = self.bus.registers.either_h_interrupt_enabled();
            let mclk_till_next_vdp_event =
                self.bus.vdp.mclk_cycles_until_next_event(h_interrupt_enabled);
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

            // SH-2s
            self.execute_cpus::<DEBUG>(debugger);

            // SH-2/SH7604 peripherals (WDT, SCI)
            let mut peripherals_bus = Sh2Bus::create(&mut self.bus, WhichCpu::Master, 0, 0, None);
            self.sh2_master.tick_peripherals(elapsed_pwm_cycles, &mut *peripherals_bus);

            peripherals_bus.which = WhichCpu::Slave;
            self.sh2_slave.tick_peripherals(elapsed_pwm_cycles, &mut *peripherals_bus);

            // 32X VDP
            self.bus.vdp.tick(mclk_cycles, &mut self.bus.registers, vdp_info);

            // PWM chip
            self.bus.pwm.tick(elapsed_pwm_cycles, &mut self.bus.registers, audio_output);
        }

        debug_assert_eq!(self.bus.vdp.scanline(), vdp_info.scanline);
        debug_assert_eq!(self.bus.vdp.scanline_mclk(), vdp_info.scanline_mclk);
    }

    fn execute_cpus<const DEBUG: bool>(&mut self, debugger: &mut impl Sega32XDebugger) {
        if self.bus.stalled_on_cartridge_access {
            self.slave_cycles = self.global_cycles;
            self.master_cycles = self.global_cycles;
            return;
        }

        // Slave SH-2
        let mut slave_bus = Sh2Bus::create(
            &mut self.bus,
            WhichCpu::Slave,
            self.slave_cycles,
            self.global_cycles,
            Some((&mut self.sh2_master, &mut self.master_cycles)),
        );

        self.slave_cycles = if DEBUG {
            let mut debug_bus = DebugSh2Bus::create(slave_bus, debugger);
            while debug_bus.cycle_counter() < debug_bus.cycle_limit() {
                self.sh2_slave.execute(SH2_EXECUTION_SLICE_LEN, &mut *debug_bus);
            }
            debug_bus.cycle_counter()
        } else {
            while slave_bus.cycle_counter < slave_bus.cycle_limit {
                self.sh2_slave.execute(SH2_EXECUTION_SLICE_LEN, &mut *slave_bus);
            }
            slave_bus.cycle_counter
        };

        // Master SH-2
        let mut master_bus = Sh2Bus::create(
            &mut self.bus,
            WhichCpu::Master,
            self.master_cycles,
            self.global_cycles,
            Some((&mut self.sh2_slave, &mut self.slave_cycles)),
        );
        self.master_cycles = if DEBUG {
            let mut debug_bus = DebugSh2Bus::create(master_bus, debugger);
            while debug_bus.cycle_counter() < debug_bus.cycle_limit() {
                self.sh2_master.execute(SH2_EXECUTION_SLICE_LEN, &mut *debug_bus);
            }
            debug_bus.cycle_counter()
        } else {
            while master_bus.cycle_counter < master_bus.cycle_limit {
                self.sh2_master.execute(SH2_EXECUTION_SLICE_LEN, &mut *master_bus);
            }
            master_bus.cycle_counter
        };
    }

    pub fn reload_config(&mut self, config: &GenesisEmulatorConfig) {
        self.sh2_clock_multiplier =
            none_if_default_multiplier(config.sega_32x.sh2_clock_multiplier);
        self.bus.vdp.reload_config(&config.sega_32x);

        if let Some(cartridge) = &mut self.bus.cartridge {
            cartridge.reload_config(config);
        }
    }

    pub fn reset(&mut self) {
        self.bus.registers.reset();
    }

    // ADEN bit in $A15100
    #[must_use]
    #[inline]
    pub fn adapter_enabled(&self) -> bool {
        self.bus.registers.adapter_enabled
    }

    // RV bit in $A15106
    #[must_use]
    #[inline]
    pub fn rom_to_vram_dma(&self) -> bool {
        self.bus.registers.dma.rom_to_vram
    }

    #[must_use]
    #[inline]
    pub fn cartridge(&self) -> Option<&Cartridge> {
        self.bus.cartridge.as_ref()
    }

    #[must_use]
    #[inline]
    pub fn cartridge_mut(&mut self) -> Option<&mut Cartridge> {
        self.bus.cartridge.as_mut()
    }

    // Reads from $000000-$3FFFFF while the 32X adapter is enabled
    #[must_use]
    #[inline]
    #[allow(clippy::missing_panics_doc)]
    pub fn m68k_read_cartridge<const WORD: bool>(&mut self, address: u32, open_bus: u16) -> u16 {
        if address < 0x000100 {
            // 68K vector ROM + HINT vector
            return if WORD {
                let address = address as usize;
                u16::from_be_bytes(self.m68k_vectors[address..address + 2].try_into().unwrap())
            } else {
                self.m68k_vectors[address as usize].into()
            };
        }

        let open_bus_read =
            move || if WORD { open_bus } else { open_bus.be_byte(address & 1).into() };

        if !self.rom_to_vram_dma() {
            // $000100-$3FFFFF is not accessible on the Genesis side while RV=0
            log::warn!("68000 invalid cartridge read while RV=0: {address:06X}");
            return open_bus_read();
        }

        let Some(cartridge) = &mut self.bus.cartridge else {
            return open_bus_read();
        };

        cartridge.read::<WORD>(address, open_bus)
    }

    // Writes to $000000-$3FFFFF while the 32X adapter is enabled
    #[inline]
    pub fn m68k_write_cartridge<const WORD: bool>(&mut self, address: u32, value: u16) {
        if (0x70..0x74).contains(&address) {
            // HINT vector is R/W
            if WORD {
                self.m68k_vectors[address as usize] = value.msb();
                self.m68k_vectors[(address + 1) as usize] = value.lsb();
            } else {
                self.m68k_vectors[address as usize] = value as u8;
            }
            return;
        }

        if !self.rom_to_vram_dma() {
            // $000100-$3FFFFF is not accessible on the Genesis side while RV=0
            log::warn!("68000 invalid cartridge write while RV=0: {address:06X} {value:04X}");
            return;
        }

        let Some(cartridge) = &mut self.bus.cartridge else { return };

        cartridge.write::<WORD>(address, value);
    }

    // Reads from $800000-$9FFFFF while the 32X adapter is enabled
    #[must_use]
    #[inline]
    pub fn m68k_read_memory<const WORD: bool>(&mut self, address: u32, open_bus: u16) -> u16 {
        match address {
            0x840000..=0x87FFFF => {
                // Frame buffer
                match self.bus.registers.vdp_access {
                    Access::M68k => {
                        let word = self.bus.vdp.read_frame_buffer(address);
                        if WORD { word } else { word.be_byte(address & 1).into() }
                    }
                    Access::Sh2 => {
                        log::warn!("Frame buffer 68000 read with FM=1: {address:06X}");
                        0xFFFF
                    }
                }
            }
            0x880000..=0x8FFFFF
                if !self.rom_to_vram_dma()
                    && let Some(cartridge) = &mut self.bus.cartridge =>
            {
                // First 512KB of cartridge
                let rom_addr = address & 0x7FFFF;
                cartridge.read::<WORD>(rom_addr, open_bus)
            }
            0x900000..=0x9FFFFF
                if !self.rom_to_vram_dma()
                    && let Some(cartridge) = &mut self.bus.cartridge =>
            {
                // Mappable 1MB cartridge bank
                let rom_addr =
                    (u32::from(self.bus.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                cartridge.read::<WORD>(rom_addr, open_bus)
            }
            _ => {
                if (0x880000..=0x9FFFFF).contains(&address) && self.rom_to_vram_dma() {
                    log::warn!("68000 invalid cartridge read while RV=1: {address:06X}");
                }

                if WORD { open_bus } else { open_bus.be_byte(address & 1).into() }
            }
        }
    }

    // Writes to $800000-$9FFFFF while the 32X adapter is enabled
    #[inline]
    pub fn m68k_write_memory<const WORD: bool>(&mut self, address: u32, value: u16) {
        match address {
            0x840000..=0x85FFFF => {
                // Frame buffer
                match self.bus.registers.vdp_access {
                    Access::M68k => {
                        if WORD {
                            self.bus.vdp.write_frame_buffer_word(address, value);
                        } else {
                            self.bus.vdp.write_frame_buffer_byte(address, value as u8);
                        }
                    }
                    Access::Sh2 => {
                        log::warn!("Frame buffer 68000 write with FM=1: {address:06X} {value:04X}");
                    }
                }
            }
            0x860000..=0x87FFFF => {
                // Frame buffer overwrite image
                match self.bus.registers.vdp_access {
                    Access::M68k => {
                        if WORD {
                            self.bus.vdp.frame_buffer_overwrite_word(address, value);
                        } else {
                            // No difference between normal and overwrite writes for byte-size
                            self.bus.vdp.write_frame_buffer_byte(address, value as u8);
                        }
                    }
                    Access::Sh2 => {
                        log::warn!("Frame buffer 68000 write with FM=1: {address:06X} {value:04X}");
                    }
                }
            }
            0x880000..=0x8FFFFF
                if !self.rom_to_vram_dma()
                    && let Some(cartridge) = &mut self.bus.cartridge =>
            {
                // First 512KB of cartridge
                let rom_addr = address & 0x7FFFF;
                cartridge.write::<WORD>(rom_addr, value);
            }
            0x900000..=0x9FFFFF
                if !self.rom_to_vram_dma()
                    && let Some(cartridge) = &mut self.bus.cartridge =>
            {
                // Mappable 1MB cartridge bank
                let rom_addr =
                    (u32::from(self.bus.registers.m68k_rom_bank) << 20) | (address & 0xFFFFF);
                cartridge.write::<WORD>(rom_addr, value);
            }
            _ => {
                if (0x880000..=0x9FFFFF).contains(&address) && self.rom_to_vram_dma() {
                    log::warn!(
                        "68000 invalid cartridge write while RV=1: {address:06X} {value:04X}"
                    );
                }
            }
        }
    }

    // Reads from $A15000-$A15FFF (32X registers and 32X CRAM)
    #[must_use]
    #[inline]
    pub fn m68k_read_register<const WORD: bool>(&mut self, address: u32, open_bus: u16) -> u16 {
        let word = match address {
            0xA15100..=0xA1512F => {
                // 32X system registers
                self.bus.registers.m68k_read(address & !1)
            }
            0xA15130..=0xA1513F => {
                // PWM
                self.bus.pwm.read_register(address & !1)
            }
            0xA15180..=0xA1518F => {
                // 32X VDP
                match self.bus.registers.vdp_access {
                    Access::M68k => self.bus.vdp.read_register(address & !1),
                    Access::Sh2 => {
                        log::warn!("68000 VDP register read while FM=1: {address:06X}");
                        !0
                    }
                }
            }
            0xA15200..=0xA153FF => {
                // 32X CRAM
                match self.bus.registers.vdp_access {
                    Access::M68k => self.bus.vdp.read_cram(address & !1),
                    Access::Sh2 => {
                        log::warn!("68000 CRAM read while FM=1: {address:06X}");
                        !0
                    }
                }
            }
            _ => open_bus,
        };

        if WORD { word } else { word.be_byte(address & 1).into() }
    }

    // Writes to $A15000-$A15FFF (32X registers and 32X CRAM)
    #[inline]
    pub fn m68k_write_register<const WORD: bool>(&mut self, address: u32, value: u16) {
        match address {
            0xA15100..=0xA1512F => {
                // 32X system registers
                if WORD {
                    self.bus.registers.m68k_write(address, value);
                } else {
                    self.bus.registers.m68k_write_byte(address, value as u8);
                }
            }
            0xA15130..=0xA1513F => {
                // PWM
                if WORD {
                    self.bus.pwm.m68k_write_register(address, value);
                } else {
                    let mut word = self.bus.pwm.read_register(address & !1);
                    if !address.bit(0) {
                        word.set_msb(value as u8);
                    } else {
                        word.set_lsb(value as u8);
                    }
                    self.bus.pwm.m68k_write_register(address & !1, word);
                }
            }
            0xA15180..=0xA1518F => {
                // 32X VDP
                match self.bus.registers.vdp_access {
                    Access::M68k => {
                        if WORD {
                            self.bus.vdp.write_register(address, value);
                        } else {
                            self.bus.vdp.write_register_byte(address, value as u8);
                        }
                    }
                    Access::Sh2 => {
                        log::warn!(
                            "68000 VDP register write while FM=1: {address:06X} {value:04X}"
                        );
                    }
                }
            }
            0xA15200..=0xA153FF => {
                // 32X CRAM
                match self.bus.registers.vdp_access {
                    Access::M68k => {
                        if WORD {
                            self.bus.vdp.write_cram(address, value);
                        } else {
                            let mut word = self.bus.vdp.read_cram(address & !1);
                            if !address.bit(0) {
                                word.set_msb(value as u8);
                            } else {
                                word.set_lsb(value as u8);
                            }
                            self.bus.vdp.write_cram(address & !1, word);
                        }
                    }
                    Access::Sh2 => {
                        log::warn!("68000 CRAM write while FM=1: {address:06X} {value:04X}");
                    }
                }
            }
            _ => {}
        }
    }

    pub fn composite_frame(&mut self, genesis_vdp: &mut GenesisVdp) {
        self.bus.vdp.composite_frame(genesis_vdp);
    }

    /// # Errors
    ///
    /// Propagates any rendering errors.
    pub fn render_frame<R: Renderer>(
        &mut self,
        genesis_vdp: &GenesisVdp,
        renderer: &mut R,
    ) -> Result<(), R::Err> {
        let frame_size = genesis_vdp.frame_size();
        let aspect_ratio = self.config.aspect_ratio.to_pixel_aspect_ratio(
            self.timing_mode,
            frame_size,
            self.config.to_gen_par_params(),
        );
        self.bus.vdp.render_frame(genesis_vdp, aspect_ratio, renderer)
    }
}

fn none_if_default_multiplier(multiplier: NonZeroU64) -> Option<NonZeroU64> {
    match multiplier.get() {
        SH2_MULTIPLIER => None,
        _ => Some(multiplier),
    }
}
