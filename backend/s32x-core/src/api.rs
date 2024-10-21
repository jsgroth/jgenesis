//! 32X public interface and main loop
//!
//! At some point common code should probably be collapsed between the Genesis/SCD/32X crates

use crate::audio::Sega32XResampler;
use crate::core::Sega32X;
use bincode::{Decode, Encode};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use genesis_core::timing::GenesisCycleCounters;
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::{Ym2612, YmTickEffect};
use genesis_core::{GenesisEmulatorConfig, GenesisInputs, GenesisRegion};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorTrait, Renderer, SaveWriter, TickEffect, TickResult, TimingMode,
};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr, PartialClone};
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgTickEffect, PsgVersion};
use std::fmt::{Debug, Display};
use std::mem;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum S32XVideoOut {
    #[default]
    Combined,
    GenesisOnly,
    S32XOnly,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct Sega32XEmulatorConfig {
    pub genesis: GenesisEmulatorConfig,
    pub video_out: S32XVideoOut,
    pub pwm_enabled: bool,
}

macro_rules! new_main_bus {
    ($self:expr, m68k_reset: $m68k_reset:expr) => {
        MainBus::new(
            &mut $self.memory,
            &mut $self.vdp,
            &mut $self.psg,
            &mut $self.ym2612,
            &mut $self.input,
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
    psg: Psg,
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
        rom: Box<[u8]>,
        config: Sega32XEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let region = config.genesis.forced_region.unwrap_or_else(|| {
            // Shadow Squadron / Stellar Assault (UE) reports its region as E in the header,
            // but it's NTSC-compatible; prefer Americas if region is not forced so it will run at
            // 60Hz instead of 50Hz
            if &rom[0x180..0x18E] == "GM MK-84509-00".as_bytes() {
                return GenesisRegion::Americas;
            }

            GenesisRegion::from_rom(&rom).unwrap_or_else(|| {
                log::error!("Unable to determine ROM region; defaulting to Americas");
                GenesisRegion::Americas
            })
        });

        let timing_mode = config.genesis.forced_timing_mode.unwrap_or(match region {
            GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
            GenesisRegion::Europe => TimingMode::Pal,
        });

        log::info!("Running with region {region:?} and timing mode {timing_mode:?}");

        let m68k = M68000::builder().allow_tas_writes(false).build();
        let z80 = Z80::new();
        let vdp = Vdp::new(timing_mode, config.genesis.to_vdp_config());
        let ym2612 = Ym2612::new(config.genesis);
        let psg = Psg::new(PsgVersion::Standard);

        let initial_cartridge_ram = save_writer.load_bytes("sav").ok();
        let s32x = Sega32X::new(rom, initial_cartridge_ram, region, timing_mode, config);
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
        genesis_core::memory::parse_title_from_header(
            &self.memory.medium().cartridge.rom,
            self.region,
        )
    }

    pub fn copy_cram(&self, out: &mut [Color]) {
        self.vdp.copy_cram(out);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        let frame_size = self.vdp.frame_size();
        let aspect_ratio = self.config.genesis.aspect_ratio.to_pixel_aspect_ratio(frame_size, true);
        self.memory.medium().vdp.render_frame(
            self.vdp.frame_buffer(),
            frame_size,
            aspect_ratio,
            renderer,
        )
    }
}

impl EmulatorTrait for Sega32XEmulator {
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
        let m68k_cycles = if self.cycles.m68k_wait_cpu_cycles != 0 {
            self.cycles.take_m68k_wait_cpu_cycles()
        } else {
            self.m68k.execute_instruction(&mut bus)
        };

        let mclk_cycles = u64::from(m68k_cycles) * self.cycles.m68k_divider.get();
        self.cycles.increment_mclk_counters(mclk_cycles);

        while self.cycles.should_tick_z80() {
            self.z80.tick(&mut bus);
            self.cycles.decrement_z80();
        }

        if bus.z80_accessed_68k_bus() {
            self.cycles.record_z80_68k_bus_access();
        }

        self.main_bus_writes = bus.apply_writes();

        self.memory.medium_mut().tick(mclk_cycles, self.audio_resampler.pwm_resampler_mut());
        self.input.tick(m68k_cycles);

        while self.cycles.should_tick_ym2612() {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                let (sample_l, sample_r) = self.ym2612.sample();
                self.audio_resampler.collect_ym2612_sample(sample_l, sample_r);
            }
            self.cycles.decrement_ym2612();
        }

        while self.cycles.should_tick_psg() {
            if self.psg.tick() == PsgTickEffect::Clocked {
                let (sample_l, sample_r) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(sample_l, sample_r);
            }
            self.cycles.decrement_psg();
        }

        self.audio_resampler.output_samples(audio_output).map_err(Sega32XError::Audio)?;

        let mut tick_effect = TickEffect::None;
        if self.vdp.tick(mclk_cycles, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.memory.medium_mut().vdp.composite_frame(
                self.vdp.frame_size(),
                self.vdp.border_size(),
                self.vdp.frame_buffer_mut(),
            );
            self.render_frame(renderer).map_err(Sega32XError::Render)?;

            let cartridge = &mut self.memory.medium_mut().cartridge;
            if cartridge.persistent_memory_dirty() {
                cartridge.clear_persistent_dirty_bit();
                save_writer
                    .persist_bytes("sav", cartridge.persistent_memory())
                    .map_err(Sega32XError::SaveWrite)?;
            }

            tick_effect = TickEffect::FrameRendered;
        }

        debug_assert_eq!(self.vdp.scanline(), self.memory.medium().vdp.scanline());
        debug_assert_eq!(self.vdp.scanline_mclk(), self.memory.medium().vdp.scanline_mclk());

        Ok(tick_effect)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.vdp.reload_config(config.genesis.to_vdp_config());
        self.ym2612.reload_config(config.genesis);
        self.input.reload_config(config.genesis);
        self.memory.medium_mut().reload_config(*config);
        self.audio_resampler.reload_config(*config);
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
        self.ym2612.reset(self.config.genesis);

        self.memory.medium_mut().reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = mem::take(&mut self.memory.medium_mut().cartridge.rom.0);

        *self = Self::create(rom, self.config, save_writer);
    }

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
