//! Genesis public interface and main loop

pub mod debug;

use crate::audio::GenesisAudioResampler;
use crate::cartridge::Cartridge;
use crate::input::InputState;
use crate::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use crate::timing::{CycleCounters, GenesisCycleCounters};
use crate::vdp::{DarkenColors, Vdp, VdpConfig, VdpTickEffect};
use crate::ym2612::Ym2612;
use crate::{audio, timing, vdp};
use bincode::{Decode, Encode};
use genesis_config::{
    GenesisAspectRatio, GenesisButton, GenesisControllerType, GenesisInputs, GenesisRegion,
    Opn2BusyBehavior,
};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorConfigTrait, EmulatorTrait, PartialClone, Renderer, SaveWriter,
    TickEffect, TimingMode,
};
use jgenesis_proc_macros::ConfigDisplay;
use m68000_emu::M68000;
use smsgg_config::Sn76489Version;
use smsgg_core::psg::{Sn76489, Sn76489TickEffect};
use std::cmp;
use std::fmt::{Debug, Display};
use std::num::NonZeroU64;
use thiserror::Error;
use z80_emu::Z80;

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

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct GenesisEmulatorConfig {
    pub p1_controller_type: GenesisControllerType,
    pub p2_controller_type: GenesisControllerType,
    pub forced_timing_mode: Option<TimingMode>,
    pub forced_region: Option<GenesisRegion>,
    pub aspect_ratio: GenesisAspectRatio,
    pub force_square_pixels_in_h40: bool,
    pub adjust_aspect_ratio_in_2x_resolution: bool,
    pub remove_sprite_limits: bool,
    pub m68k_clock_divider: u64,
    pub non_linear_color_scale: bool,
    pub deinterlace: bool,
    pub render_vertical_border: bool,
    pub render_horizontal_border: bool,
    pub plane_a_enabled: bool,
    pub plane_b_enabled: bool,
    pub sprites_enabled: bool,
    pub window_enabled: bool,
    pub backdrop_enabled: bool,
    pub quantize_ym2612_output: bool,
    pub emulate_ym2612_ladder_effect: bool,
    pub opn2_busy_behavior: Opn2BusyBehavior,
    pub genesis_lpf_enabled: bool,
    pub genesis_lpf_cutoff: u32,
    pub ym2612_2nd_lpf_enabled: bool,
    pub ym2612_2nd_lpf_cutoff: u32,
    #[cfg_display(debug_fmt)]
    pub ym2612_channels_enabled: [bool; 6],
    pub ym2612_enabled: bool,
    pub psg_enabled: bool,
    pub ym2612_volume_adjustment_db: f64,
    pub psg_volume_adjustment_db: f64,
}

impl GenesisEmulatorConfig {
    #[must_use]
    pub fn to_vdp_config(&self, color_adjustment: DarkenColors) -> VdpConfig {
        VdpConfig {
            enforce_sprite_limits: !self.remove_sprite_limits,
            non_linear_color_scale: self.non_linear_color_scale,
            deinterlace: self.deinterlace,
            render_vertical_border: self.render_vertical_border,
            render_horizontal_border: self.render_horizontal_border,
            plane_a_enabled: self.plane_a_enabled,
            plane_b_enabled: self.plane_b_enabled,
            sprites_enabled: self.sprites_enabled,
            window_enabled: self.window_enabled,
            backdrop_enabled: self.backdrop_enabled,
            color_adjustment,
        }
    }

    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    pub fn clamped_m68k_divider(&self) -> NonZeroU64 {
        let clamped_divider = self.m68k_clock_divider.clamp(1, timing::NATIVE_M68K_DIVIDER);
        if clamped_divider != self.m68k_clock_divider {
            log::warn!(
                "Clamped M68K clock divider from {} to {clamped_divider}",
                self.m68k_clock_divider
            );
        }
        NonZeroU64::new(clamped_divider).unwrap()
    }
}

impl EmulatorConfigTrait for GenesisEmulatorConfig {
    fn with_overclocking_disabled(&self) -> Self {
        Self { m68k_clock_divider: timing::NATIVE_M68K_DIVIDER, ..*self }
    }
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct GenesisEmulator {
    #[partial_clone(partial)]
    memory: Memory<Cartridge>,
    m68k: M68000,
    z80: Z80,
    vdp: Vdp,
    psg: Sn76489,
    ym2612: Ym2612,
    input: InputState,
    timing_mode: TimingMode,
    main_bus_writes: MainBusWrites,
    audio_resampler: GenesisAudioResampler,
    cycles: GenesisCycleCounters,
    config: GenesisEmulatorConfig,
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
            &mut $self.cycles,
            $self.m68k.next_opcode(),
            $self.timing_mode,
            MainBusSignals { m68k_reset: $m68k_reset },
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
        let vdp = Vdp::new(timing_mode, config.to_vdp_config(DarkenColors::No));
        let psg = Sn76489::new(Sn76489Version::Standard);
        let ym2612 = Ym2612::new_from_config(&config);
        let input = InputState::new(config.p1_controller_type, config.p2_controller_type);

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
            audio_resampler: GenesisAudioResampler::new(timing_mode, config),
            cycles: GenesisCycleCounters::new(config.clamped_m68k_divider()),
            config,
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

    #[inline]
    #[must_use]
    pub fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        render_frame(self.timing_mode, &self.vdp, &self.config, renderer)
    }
}

/// Render the current VDP frame buffer.
///
/// # Errors
///
/// This function will propagate any error returned by the renderer.
pub fn render_frame<R: Renderer>(
    timing_mode: TimingMode,
    vdp: &Vdp,
    config: &GenesisEmulatorConfig,
    renderer: &mut R,
) -> Result<(), R::Err> {
    let frame_size = vdp.frame_size();
    let pixel_aspect_ratio = config.aspect_ratio.to_pixel_aspect_ratio(
        timing_mode,
        frame_size,
        config.force_square_pixels_in_h40,
        config.adjust_aspect_ratio_in_2x_resolution,
    );

    renderer.render_frame(vdp.frame_buffer(), frame_size, pixel_aspect_ratio)
}

impl EmulatorTrait for GenesisEmulator {
    type Button = GenesisButton;
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
        let m68k_pc = self.m68k.pc();
        let m68k_wait = bus.cycles.m68k_wait_cpu_cycles != 0;
        let m68k_cycles = if m68k_wait {
            bus.cycles.take_m68k_wait_cpu_cycles()
        } else {
            self.m68k.execute_instruction(&mut bus)
        };

        let elapsed_mclk_cycles = bus.cycles.record_68k_instruction(
            m68k_pc,
            m68k_cycles,
            m68k_wait,
            bus.vdp.should_halt_cpu(),
        );

        while bus.cycles.should_tick_z80() {
            if !bus.cycles.z80_halt {
                self.z80.tick(&mut bus);
            }
            bus.cycles.decrement_z80();
        }

        self.main_bus_writes = bus.pending_writes;

        self.memory.medium_mut().tick(m68k_cycles);

        self.input.tick(m68k_cycles);

        while self.cycles.should_tick_psg() {
            if self.psg.tick() == Sn76489TickEffect::Clocked {
                // PSG only has mono output in the Genesis; stereo output is only for Game Gear
                let (psg_sample, _) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(psg_sample);
            }

            self.cycles.decrement_psg();
        }

        if self.cycles.has_ym2612_ticks() {
            let ym2612_ticks = self.cycles.take_ym2612_ticks();
            self.ym2612
                .tick(ym2612_ticks, |(l, r)| self.audio_resampler.collect_ym2612_sample(l, r));
        }

        self.audio_resampler.output_samples(audio_output).map_err(GenesisError::Audio)?;

        let mut tick_effect = TickEffect::None;
        if self.vdp.tick(elapsed_mclk_cycles, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(GenesisError::Render)?;

            self.input.set_inputs(*inputs);

            if self.memory.is_external_ram_persistent()
                && self.memory.get_and_clear_external_ram_dirty()
            {
                let ram = self.memory.external_ram();
                if !ram.is_empty() {
                    save_writer.persist_bytes("sav", ram).map_err(GenesisError::Save)?;
                }
            }

            tick_effect = TickEffect::FrameRendered;
        }

        check_for_long_dma_skip(&self.vdp, &mut self.cycles);

        if !m68k_wait {
            self.vdp.update_interrupt_latches();
        }

        self.main_bus_writes = new_main_bus!(self, m68k_reset: false).apply_writes();

        Ok(tick_effect)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.vdp.reload_config(config.to_vdp_config(DarkenColors::No));
        self.ym2612.reload_config(*config);
        self.input.reload_config(*config);
        self.audio_resampler.reload_config(self.timing_mode, *config);
        self.cycles.update_m68k_divider(config.clamped_m68k_divider());

        self.config = *config;
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
        *self = GenesisEmulator::create(rom, self.config, save_writer);
    }

    fn target_fps(&self) -> f64 {
        target_framerate(&self.vdp, self.timing_mode)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }
}

#[inline]
#[must_use]
pub fn target_framerate(vdp: &Vdp, timing_mode: TimingMode) -> f64 {
    let mclk_frequency = match timing_mode {
        TimingMode::Ntsc => audio::NTSC_GENESIS_MCLK_FREQUENCY,
        TimingMode::Pal => audio::PAL_GENESIS_MCLK_FREQUENCY,
    };

    mclk_frequency / (vdp::MCLK_CYCLES_PER_SCANLINE as f64) / vdp.average_scanlines_per_frame()
}

// If a long DMA is in progress (i.e. the DMA will not finish on this line), preemptively skip the
// 68000 forward by a large number of mclk cycles (up to 1250).
//
// This function is public so that it can be used by the Sega CD core
#[inline]
pub fn check_for_long_dma_skip<const REFRESH_INTERVAL: u32>(
    vdp: &Vdp,
    cycles: &mut CycleCounters<REFRESH_INTERVAL>,
) {
    if !vdp.long_halting_dma_in_progress() {
        return;
    }

    if !cycles.z80_halt {
        // Don't advance for very long time slices if the Z80 is still active; doing so causes
        // video/audio desync in Overdrive 2.
        // 8 68K cycles is slightly less than 4 Z80 cycles
        cycles.m68k_wait_cpu_cycles = 8;
        return;
    }

    // Skip as close as possible to the end of the current scanline
    let wait_cycles = cmp::max(
        cycles.m68k_wait_cpu_cycles,
        cmp::min(
            cycles.max_wait_cpu_cycles,
            (vdp::MCLK_CYCLES_PER_SCANLINE - vdp.scanline_mclk()) as u32
                / cycles.m68k_divider_u32.get(),
        ),
    );
    cycles.m68k_wait_cpu_cycles = wait_cycles;

    log::trace!(
        "Skipping {wait_cycles} 68000 CPU cycles in long DMA optimization, scanline mclk is {}",
        vdp.scanline_mclk()
    );
}
