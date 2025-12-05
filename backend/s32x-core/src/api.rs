//! 32X public interface and main loop
//!
//! At some point common code should probably be collapsed between the Genesis/SCD/32X crates

pub mod debug;

use crate::audio::Sega32XResampler;
use crate::core::Sega32X;
use bincode::{Decode, Encode};
use genesis_config::{GenesisButton, GenesisRegion, S32XColorTint, S32XVideoOut, S32XVoidColor};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use genesis_core::timing::GenesisCycleCounters;
use genesis_core::vdp::{DarkenColors, Vdp, VdpTickEffect};
use genesis_core::ym2612::Ym2612;
use genesis_core::{GenesisEmulatorConfig, GenesisInputs};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorConfigTrait, EmulatorTrait, Renderer, SaveWriter, TickEffect, TickResult,
    TimingMode,
};
use jgenesis_proc_macros::{ConfigDisplay, PartialClone};
use m68000_emu::M68000;
use smsgg_config::Sn76489Version;
use smsgg_core::psg::{Sn76489, Sn76489TickEffect};
use std::fmt::{Debug, Display};
use std::num::NonZeroU64;
use thiserror::Error;
use z80_emu::Z80;

#[derive(Debug, Error)]
pub enum Sega32XError<RErr, AErr, SErr> {
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct Sega32XEmulatorConfig {
    #[cfg_display(skip)]
    pub genesis: GenesisEmulatorConfig,
    pub sh2_clock_multiplier: NonZeroU64,
    pub video_out: S32XVideoOut,
    pub darken_genesis_colors: bool,
    pub color_tint: S32XColorTint,
    pub show_high_priority: bool,
    pub show_low_priority: bool,
    pub void_color: S32XVoidColor,
    pub apply_genesis_lpf_to_pwm: bool,
    pub pwm_enabled: bool,
    pub pwm_volume_adjustment_db: f64,
}

impl Sega32XEmulatorConfig {
    fn genesis_color_adjustment(&self) -> DarkenColors {
        if self.darken_genesis_colors { DarkenColors::Yes } else { DarkenColors::No }
    }
}

impl EmulatorConfigTrait for Sega32XEmulatorConfig {
    fn with_overclocking_disabled(&self) -> Self {
        Self {
            genesis: self.genesis.with_overclocking_disabled(),
            sh2_clock_multiplier: NonZeroU64::new(crate::SH2_CLOCK_MULTIPLIER).unwrap(),
            ..*self
        }
    }
}

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

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32XEmulator {
    m68k: M68000,
    z80: Z80,
    vdp: Vdp,
    ym2612: Ym2612,
    psg: Sn76489,
    #[partial_clone(partial)]
    memory: Memory<Sega32X>,
    input: InputState,
    audio_resampler: Sega32XResampler,
    main_bus_writes: MainBusWrites,
    cycles: GenesisCycleCounters,
    region: GenesisRegion,
    timing_mode: TimingMode,
    config: Sega32XEmulatorConfig,
}

impl Sega32XEmulator {
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        config: Sega32XEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let initial_cartridge_ram = save_writer.load_bytes("sav").ok();
        let s32x = Sega32X::new(rom, initial_cartridge_ram, &config);

        let region = s32x.region;
        let timing_mode = s32x.timing_mode;

        log::info!("Running with region {region:?} and timing mode {timing_mode:?}");

        let m68k = M68000::builder().allow_tas_writes(false).build();
        let z80 = Z80::new();
        let vdp =
            Vdp::new(timing_mode, config.genesis.to_vdp_config(config.genesis_color_adjustment()));
        let ym2612 = Ym2612::new_from_config(&config.genesis);
        let psg = Sn76489::new(Sn76489Version::Standard);

        let memory = Memory::new(s32x);

        let input =
            InputState::new(config.genesis.p1_controller_type, config.genesis.p2_controller_type);

        let mut emulator = Self {
            m68k,
            z80,
            vdp,
            ym2612,
            psg,
            memory,
            input,
            audio_resampler: Sega32XResampler::new(timing_mode, config),
            main_bus_writes: MainBusWrites::new(),
            cycles: GenesisCycleCounters::new(config.genesis.clamped_m68k_divider()),
            region,
            timing_mode,
            config,
        };

        emulator.m68k.execute_instruction(&mut new_main_bus!(emulator, m68k_reset: true));

        emulator
    }

    #[must_use]
    pub fn cartridge_title(&self) -> String {
        self.memory.medium().cartridge().program_title().into()
    }

    #[inline]
    #[must_use]
    pub fn has_sram(&self) -> bool {
        self.memory.medium().cartridge().is_ram_persistent()
    }

    #[inline]
    #[must_use]
    pub fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        let frame_size = self.vdp.frame_size();
        let aspect_ratio = self.config.genesis.aspect_ratio.to_pixel_aspect_ratio(
            self.timing_mode,
            frame_size,
            self.config.genesis.force_square_pixels_in_h40,
            self.config.genesis.adjust_aspect_ratio_in_2x_resolution,
        );
        self.memory.medium_mut().vdp().render_frame(&self.vdp, aspect_ratio, renderer)
    }
}

impl EmulatorTrait for Sega32XEmulator {
    type Button = GenesisButton;
    type Inputs = GenesisInputs;
    type Config = Sega32XEmulatorConfig;
    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = Sega32XError<RErr, AErr, SErr>;

    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> TickResult<Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        R::Err: Debug + Display + Send + Sync + 'static,
        A: AudioOutput,
        A::Err: Debug + Display + Send + Sync + 'static,
        S: SaveWriter,
        S::Err: Debug + Display + Send + Sync + 'static,
    {
        self.input.set_inputs(*inputs);

        let mut bus = new_main_bus!(self, m68k_reset: false);
        let m68k_wait = bus.cycles.m68k_wait_cpu_cycles != 0;
        let m68k_cycles = if m68k_wait {
            bus.cycles.take_m68k_wait_cpu_cycles()
        } else {
            self.m68k.execute_instruction(&mut bus)
        };

        let mclk_cycles = u64::from(m68k_cycles) * bus.cycles.m68k_divider.get();
        bus.cycles.increment_mclk_counters(mclk_cycles, bus.vdp.should_halt_cpu());

        while bus.cycles.should_tick_z80() {
            if !bus.cycles.z80_halt {
                self.z80.tick(&mut bus);
            }
            bus.cycles.decrement_z80();
        }

        self.main_bus_writes = bus.take_writes();

        self.memory.medium_mut().tick(
            mclk_cycles,
            self.audio_resampler.pwm_resampler_mut(),
            &self.vdp,
        );
        self.input.tick(m68k_cycles);

        if self.cycles.has_ym2612_ticks() {
            let ym2612_ticks = self.cycles.take_ym2612_ticks();
            self.ym2612
                .tick(ym2612_ticks, |(l, r)| self.audio_resampler.collect_ym2612_sample(l, r));
        }

        while self.cycles.should_tick_psg() {
            if self.psg.tick() == Sn76489TickEffect::Clocked {
                // PSG output is mono in Genesis; stereo output is only for Game Gear
                let (psg_sample, _) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(psg_sample);
            }
            self.cycles.decrement_psg();
        }

        self.audio_resampler.output_samples(audio_output).map_err(Sega32XError::Audio)?;

        let mut tick_effect = TickEffect::None;
        if self.vdp.tick(mclk_cycles, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.memory.medium_mut().vdp().composite_frame(&mut self.vdp);
            self.render_frame(renderer).map_err(Sega32XError::Render)?;

            let cartridge = self.memory.medium_mut().cartridge_mut();
            if cartridge.get_and_clear_ram_dirty() {
                save_writer
                    .persist_bytes("sav", cartridge.external_ram())
                    .map_err(Sega32XError::SaveWrite)?;
            }

            tick_effect = TickEffect::FrameRendered;
        }

        debug_assert_eq!(self.vdp.scanline(), self.memory.medium_mut().vdp().scanline());
        debug_assert_eq!(self.vdp.scanline_mclk(), self.memory.medium_mut().vdp().scanline_mclk());

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
        self.vdp.reload_config(config.genesis.to_vdp_config(config.genesis_color_adjustment()));
        self.ym2612.reload_config(config.genesis);
        self.input.reload_config(config.genesis);
        self.memory.medium_mut().reload_config(config);
        self.audio_resampler.reload_config(self.timing_mode, *config);
        self.cycles.update_m68k_divider(config.genesis.clamped_m68k_divider());

        self.config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.medium_mut().take_rom_from(other.memory.medium_mut());
    }

    fn soft_reset(&mut self) {
        log::info!("Soft resetting console");

        self.m68k.execute_instruction(&mut new_main_bus!(self, m68k_reset: true));
        self.memory.reset_z80_signals();
        self.ym2612.reset();

        self.memory.medium_mut().reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = self.memory.medium_mut().cartridge_mut().take_rom();

        *self = Self::create(rom, self.config, save_writer);
    }

    fn target_fps(&self) -> f64 {
        genesis_core::target_framerate(&self.vdp, self.timing_mode)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }
}
