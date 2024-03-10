//! Genesis public interface and main loop

use crate::audio::GenesisAudioResampler;
use crate::input::{GenesisInputs, InputState};
use crate::memory::{Cartridge, MainBus, MainBusSignals, MainBusWrites, Memory};
use crate::vdp::{Vdp, VdpConfig, VdpTickEffect};
use crate::ym2612::{Ym2612, YmTickEffect};
use crate::GenesisControllerType;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorTrait, FrameSize, PartialClone, PixelAspectRatio, Renderer,
    SaveWriter, TickEffect, TimingMode,
};
use jgenesis_common::num::GetBit;
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr};
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgTickEffect, PsgVersion};
use std::fmt::{Debug, Display};
use std::mem;
use thiserror::Error;
use z80_emu::Z80;

const M68K_MCLK_DIVIDER: u64 = 7;
const Z80_MCLK_DIVIDER: u64 = 15;
const PSG_MCLK_DIVIDER: u64 = 15;

#[derive(Debug, Error)]
pub enum GenesisError<RErr, AErr, SErr> {
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio output error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    Save(SErr),
}

pub type GenesisResult<RErr, AErr, SErr> = Result<TickEffect, GenesisError<RErr, AErr, SErr>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GenesisAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl GenesisAspectRatio {
    fn to_pixel_aspect_ratio(
        self,
        frame_size: FrameSize,
        adjust_for_2x_resolution: bool,
    ) -> Option<PixelAspectRatio> {
        let mut pixel_aspect_ratio = match (self, frame_size.width) {
            (Self::SquarePixels, _) => Some(1.0),
            (Self::Stretched, _) => None,
            (Self::Ntsc, 256..=284) => Some(8.0 / 7.0),
            (Self::Ntsc, 320..=347) => Some(32.0 / 35.0),
            (Self::Pal, 256..=284) => Some(11.0 / 8.0),
            (Self::Pal, 320..=347) => Some(11.0 / 10.0),
            (Self::Ntsc | Self::Pal, _) => {
                panic!("unexpected Genesis frame width: {}", frame_size.width)
            }
        };

        if adjust_for_2x_resolution && frame_size.height >= 448 {
            pixel_aspect_ratio = pixel_aspect_ratio.map(|par| par * 2.0);
        }

        pixel_aspect_ratio.map(|par| PixelAspectRatio::try_from(par).unwrap())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumDisplay, EnumFromStr, Encode, Decode)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GenesisRegion {
    Americas,
    Japan,
    Europe,
}

impl GenesisRegion {
    #[must_use]
    pub fn from_rom(rom: &[u8]) -> Option<Self> {
        let region_bytes = &rom[0x1F0..0x1F3];

        // Prefer Americas if region code contains a 'U'
        if region_bytes.contains(&b'U') {
            return Some(GenesisRegion::Americas);
        }

        // Otherwise, prefer Japan if it contains a 'J'
        if region_bytes.contains(&b'J') {
            return Some(GenesisRegion::Japan);
        }

        // Finally, prefer Europe if it contains an 'E'
        if region_bytes.contains(&b'E') {
            return Some(GenesisRegion::Europe);
        }

        // If region code contains neither a 'U' nor a 'J', treat it as a hex char
        let c = region_bytes[0] as char;
        let value = u8::from_str_radix(&c.to_string(), 16).ok()?;
        if value.bit(2) {
            // Bit 2 = Americas
            Some(GenesisRegion::Americas)
        } else if value.bit(0) {
            // Bit 0 = Asia
            Some(GenesisRegion::Japan)
        } else if value.bit(3) {
            // Bit 3 = Europe
            Some(GenesisRegion::Europe)
        } else {
            // Invalid
            None
        }
    }

    #[must_use]
    pub fn version_bit(self) -> bool {
        self != Self::Japan
    }
}

#[derive(Debug, Clone, Copy)]
pub struct GenesisEmulatorConfig {
    pub p1_controller_type: GenesisControllerType,
    pub p2_controller_type: GenesisControllerType,
    pub forced_timing_mode: Option<TimingMode>,
    pub forced_region: Option<GenesisRegion>,
    pub aspect_ratio: GenesisAspectRatio,
    pub adjust_aspect_ratio_in_2x_resolution: bool,
    pub remove_sprite_limits: bool,
    pub emulate_non_linear_vdp_dac: bool,
    pub render_vertical_border: bool,
    pub render_horizontal_border: bool,
    pub quantize_ym2612_output: bool,
}

impl GenesisEmulatorConfig {
    #[must_use]
    pub fn to_vdp_config(self) -> VdpConfig {
        VdpConfig {
            enforce_sprite_limits: !self.remove_sprite_limits,
            emulate_non_linear_dac: self.emulate_non_linear_vdp_dac,
            render_vertical_border: self.render_vertical_border,
            render_horizontal_border: self.render_horizontal_border,
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode)]
struct WaitStates {
    m68k_cpu_cycles: u32,
    z80_mclk_cycles: u64,
    odd_access: bool,
}

impl WaitStates {
    fn handle_z80_68k_bus_access(&mut self) {
        // Each time the Z80 accesses the 68K bus, the Z80 is stalled for on average 3.3 Z80 cycles (= 49.5 mclk cycles)
        // and the 68K is stalled for on average 11 68K cycles
        self.m68k_cpu_cycles = 11;
        self.z80_mclk_cycles = 49 + u64::from(self.odd_access);
        self.odd_access = !self.odd_access;
    }
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct GenesisEmulator {
    #[partial_clone(partial)]
    memory: Memory<Cartridge>,
    m68k: M68000,
    z80: Z80,
    vdp: Vdp,
    psg: Psg,
    ym2612: Ym2612,
    input: InputState,
    timing_mode: TimingMode,
    main_bus_writes: MainBusWrites,
    aspect_ratio: GenesisAspectRatio,
    adjust_aspect_ratio_in_2x_resolution: bool,
    audio_resampler: GenesisAudioResampler,
    z80_mclk_cycles: u64,
    psg_mclk_cycles: u64,
    wait_states: WaitStates,
}

// This is a macro instead of a function so that it only mutably borrows the needed fields
macro_rules! new_main_bus {
    ($self:expr, m68k_reset: $m68k_reset:expr) => {
        MainBus::new(
            &mut $self.memory,
            &mut $self.vdp,
            &mut $self.psg,
            &mut $self.ym2612,
            &mut $self.input,
            $self.timing_mode,
            MainBusSignals { z80_busack: $self.z80.stalled(), m68k_reset: $m68k_reset },
            std::mem::take(&mut $self.main_bus_writes),
        )
    };
}

impl GenesisEmulator {
    /// Initialize the emulator from the given ROM.
    ///
    /// # Errors
    ///
    /// Returns an error if unable to parse the ROM header.
    #[must_use]
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        config: GenesisEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let initial_ram = save_writer.load_bytes("sav").ok();
        let cartridge = Cartridge::from_rom(rom, initial_ram, config.forced_region);
        let memory = Memory::new(cartridge);

        let timing_mode =
            config.forced_timing_mode.unwrap_or_else(|| match memory.hardware_region() {
                GenesisRegion::Europe => TimingMode::Pal,
                GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
            });

        log::info!("Using timing / display mode {timing_mode}");

        let z80 = Z80::new();
        let vdp = Vdp::new(timing_mode, config.to_vdp_config());
        let psg = Psg::new(PsgVersion::Standard);
        let ym2612 = Ym2612::new(config.quantize_ym2612_output);
        let input = InputState::new();

        // The Genesis does not allow TAS to lock the bus, so don't allow TAS writes
        let m68k = M68000::builder().allow_tas_writes(false).build();

        let mut emulator = Self {
            memory,
            m68k,
            z80,
            vdp,
            psg,
            ym2612,
            input,
            timing_mode,
            main_bus_writes: MainBusWrites::new(),
            aspect_ratio: config.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: config.adjust_aspect_ratio_in_2x_resolution,
            audio_resampler: GenesisAudioResampler::new(timing_mode),
            z80_mclk_cycles: 0,
            psg_mclk_cycles: 0,
            wait_states: WaitStates::default(),
        };

        // Reset CPU so that execution will start from the right place
        emulator.m68k.execute_instruction(&mut new_main_bus!(emulator, m68k_reset: true));

        emulator
    }

    #[must_use]
    pub fn cartridge_title(&self) -> String {
        self.memory.game_title()
    }

    #[inline]
    #[must_use]
    pub fn has_sram(&self) -> bool {
        self.memory.is_external_ram_persistent()
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        render_frame(
            &self.vdp,
            self.aspect_ratio,
            self.adjust_aspect_ratio_in_2x_resolution,
            renderer,
        )
    }

    pub fn copy_cram(&self, out: &mut [Color]) {
        self.vdp.copy_cram(out);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }
}

/// Render the current VDP frame buffer.
///
/// # Errors
///
/// This function will propagate any error returned by the renderer.
pub fn render_frame<R: Renderer>(
    vdp: &Vdp,
    aspect_ratio: GenesisAspectRatio,
    adjust_aspect_ratio_in_2x_resolution: bool,
    renderer: &mut R,
) -> Result<(), R::Err> {
    let frame_width = vdp.screen_width();
    let frame_height = vdp.screen_height();

    let frame_size = FrameSize { width: frame_width, height: frame_height };
    let pixel_aspect_ratio =
        aspect_ratio.to_pixel_aspect_ratio(frame_size, adjust_aspect_ratio_in_2x_resolution);

    renderer.render_frame(vdp.frame_buffer(), frame_size, pixel_aspect_ratio)
}

impl EmulatorTrait for GenesisEmulator {
    type Inputs = GenesisInputs;
    type Config = GenesisEmulatorConfig;

    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = GenesisError<RErr, AErr, SErr>;

    /// Execute one 68000 CPU instruction and run the rest of the components for the appropriate
    /// number of cycles.
    ///
    /// # Errors
    ///
    /// This method will propagate any errors encountered while rendering frames or pushing audio
    /// samples.
    #[inline]
    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> GenesisResult<R::Err, A::Err, S::Err>
    where
        R: Renderer,
        R::Err: Debug + Display + Send + Sync + 'static,
        A: AudioOutput,
        A::Err: Debug + Display + Send + Sync + 'static,
        S: SaveWriter,
        S::Err: Debug + Display + Send + Sync + 'static,
    {
        let mut bus = new_main_bus!(self, m68k_reset: false);
        let m68k_cycles = if self.wait_states.m68k_cpu_cycles != 0 {
            mem::take(&mut self.wait_states.m68k_cpu_cycles)
        } else {
            self.m68k.execute_instruction(&mut bus)
        };

        let elapsed_mclk_cycles = u64::from(m68k_cycles) * M68K_MCLK_DIVIDER;

        self.z80_mclk_cycles += elapsed_mclk_cycles;
        if self.z80_mclk_cycles >= self.wait_states.z80_mclk_cycles {
            self.z80_mclk_cycles -= self.wait_states.z80_mclk_cycles;
            self.wait_states.z80_mclk_cycles = 0;
        } else {
            self.wait_states.z80_mclk_cycles -= self.z80_mclk_cycles;
            self.z80_mclk_cycles = 0;
        }

        while self.z80_mclk_cycles >= Z80_MCLK_DIVIDER {
            self.z80.tick(&mut bus);
            self.z80_mclk_cycles -= Z80_MCLK_DIVIDER;
        }

        if bus.z80_accessed_68k_bus() {
            self.wait_states.handle_z80_68k_bus_access();
        }

        self.main_bus_writes = bus.apply_writes();

        self.memory.medium_mut().tick(m68k_cycles);

        self.input.tick(m68k_cycles);

        self.psg_mclk_cycles += elapsed_mclk_cycles;
        while self.psg_mclk_cycles >= PSG_MCLK_DIVIDER {
            if self.psg.tick() == PsgTickEffect::Clocked {
                let (psg_sample_l, psg_sample_r) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(psg_sample_l, psg_sample_r);
            }

            self.psg_mclk_cycles -= PSG_MCLK_DIVIDER;
        }

        // The YM2612 uses the same master clock divider as the 68000
        for _ in 0..m68k_cycles {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                let (ym_sample_l, ym_sample_r) = self.ym2612.sample();
                self.audio_resampler.collect_ym2612_sample(ym_sample_l, ym_sample_r);
            }
        }

        if self.vdp.tick(elapsed_mclk_cycles, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(GenesisError::Render)?;

            self.audio_resampler.output_samples(audio_output).map_err(GenesisError::Audio)?;

            self.input.set_inputs(*inputs);

            if self.memory.is_external_ram_persistent()
                && self.memory.get_and_clear_external_ram_dirty()
            {
                let ram = self.memory.external_ram();
                if !ram.is_empty() {
                    save_writer.persist_bytes("sav", ram).map_err(GenesisError::Save)?;
                }
            }

            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.aspect_ratio = config.aspect_ratio;
        self.adjust_aspect_ratio_in_2x_resolution = config.adjust_aspect_ratio_in_2x_resolution;
        self.vdp.reload_config(config.to_vdp_config());
        self.ym2612.set_quantize_output(config.quantize_ym2612_output);
        self.input.reload_config(*config);
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.take_rom_from(&mut other.memory);
    }

    fn soft_reset(&mut self) {
        log::info!("Soft resetting console");

        self.m68k.execute_instruction(&mut new_main_bus!(self, m68k_reset: true));
        self.memory.reset_z80_signals();
        self.ym2612.reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        log::info!("Hard resetting console");

        let rom = self.memory.take_rom();
        let vdp_config = self.vdp.config();
        let (p1_controller_type, p2_controller_type) = self.input.controller_types();

        let config = GenesisEmulatorConfig {
            forced_timing_mode: Some(self.timing_mode),
            forced_region: Some(self.memory.hardware_region()),
            aspect_ratio: self.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: self.adjust_aspect_ratio_in_2x_resolution,
            remove_sprite_limits: !vdp_config.enforce_sprite_limits,
            emulate_non_linear_vdp_dac: vdp_config.emulate_non_linear_dac,
            render_vertical_border: vdp_config.render_vertical_border,
            render_horizontal_border: vdp_config.render_horizontal_border,
            quantize_ym2612_output: self.ym2612.get_quantize_output(),
            p1_controller_type,
            p2_controller_type,
        };

        *self = GenesisEmulator::create(rom, config, save_writer);
    }

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
