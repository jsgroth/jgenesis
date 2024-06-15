//! 32X public interface and main loop
//!
//! At some point common code should probably be collapsed between the Genesis/SCD/32X crates

use crate::audio::Sega32XResampler;
use crate::core::Sega32X;
use bincode::{Decode, Encode};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::{Ym2612, YmTickEffect};
use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig, GenesisInputs, GenesisRegion};
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

pub(crate) const M68K_DIVIDER: u64 = 7;
const Z80_DIVIDER: u64 = 15;

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
            MainBusSignals { z80_busack: $self.z80.stalled(), m68k_reset: $m68k_reset },
            std::mem::take(&mut $self.main_bus_writes),
        )
    };
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct Sega32XEmulator {
    m68k: M68000,
    z80: Z80,
    z80_mclk_cycles: u64,
    vdp: Vdp,
    ym2612: Ym2612,
    psg: Psg,
    #[partial_clone(partial)]
    memory: Memory<Sega32X>,
    input: InputState,
    audio_resampler: Sega32XResampler,
    main_bus_writes: MainBusWrites,
    timing_mode: TimingMode,
    config: Sega32XEmulatorConfig,
}

impl Sega32XEmulator {
    pub fn create<S: SaveWriter>(
        rom: Box<[u8]>,
        config: Sega32XEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        let m68k = M68000::builder().allow_tas_writes(false).build();
        let z80 = Z80::new();
        // TODO
        let timing_mode = TimingMode::Ntsc;
        let vdp = Vdp::new(timing_mode, config.genesis.to_vdp_config());
        let ym2612 = Ym2612::new(config.genesis);
        let psg = Psg::new(PsgVersion::Standard);

        let initial_cartridge_ram = save_writer.load_bytes("sav").ok();
        let s32x = Sega32X::new(rom, initial_cartridge_ram, timing_mode, config.video_out);
        let memory = Memory::new(s32x);

        let input =
            InputState::new(config.genesis.p1_controller_type, config.genesis.p2_controller_type);

        let mut emulator = Self {
            m68k,
            z80,
            z80_mclk_cycles: 0,
            vdp,
            ym2612,
            psg,
            memory,
            input,
            audio_resampler: Sega32XResampler::new(timing_mode),
            main_bus_writes: MainBusWrites::new(),
            timing_mode,
            config,
        };

        emulator.m68k.execute_instruction(&mut new_main_bus!(emulator, m68k_reset: true));

        emulator
    }

    #[must_use]
    pub fn cartridge_title(&self) -> String {
        // TODO don't hardcode region
        genesis_core::memory::parse_title_from_header(
            &self.memory.medium().cartridge.rom,
            GenesisRegion::Americas,
        )
    }

    pub fn copy_cram(&self, out: &mut [Color]) {
        self.vdp.copy_cram(out);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        // TODO
        genesis_core::render_frame(&self.vdp, GenesisAspectRatio::Ntsc, true, renderer)
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
        let m68k_cycles: u64 = self.m68k.execute_instruction(&mut bus).into();

        let mclk_cycles = M68K_DIVIDER * m68k_cycles;
        self.z80_mclk_cycles += mclk_cycles;

        let mut z80_cycles = 0;
        while self.z80_mclk_cycles >= Z80_DIVIDER {
            self.z80.tick(&mut bus);
            self.z80_mclk_cycles -= Z80_DIVIDER;
            z80_cycles += 1;
        }

        self.main_bus_writes = bus.apply_writes();

        self.memory.medium_mut().tick(m68k_cycles, self.audio_resampler.pwm_resampler_mut());
        self.input.tick(m68k_cycles as u32);

        for _ in 0..m68k_cycles {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                let (sample_l, sample_r) = self.ym2612.sample();
                self.audio_resampler.collect_ym2612_sample(sample_l, sample_r);
            }
        }

        for _ in 0..z80_cycles {
            if self.psg.tick() == PsgTickEffect::Clocked {
                let (sample_l, sample_r) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(sample_l, sample_r);
            }
        }

        self.audio_resampler.output_samples(audio_output).map_err(Sega32XError::Audio)?;

        let mut tick_effect = TickEffect::None;
        if self.vdp.tick(mclk_cycles, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.memory.medium().vdp.composite_frame(
                self.vdp.frame_size(),
                self.vdp.border_size(),
                self.vdp.frame_buffer_mut(),
            );
            self.render_frame(renderer).map_err(Sega32XError::Render)?;

            if let Some(cartridge_ram) = &mut self.memory.medium_mut().cartridge.ram {
                if cartridge_ram.dirty {
                    save_writer
                        .persist_bytes("sav", &cartridge_ram.ram)
                        .map_err(Sega32XError::SaveWrite)?;
                    cartridge_ram.dirty = false;
                }
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
        self.memory.medium_mut().vdp.update_video_out(config.video_out);

        self.config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.medium_mut().take_rom_from(other.memory.medium_mut());
    }

    fn soft_reset(&mut self) {
        todo!("soft reset")
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = mem::take(&mut self.memory.medium_mut().cartridge.rom.0);

        *self = Self::create(rom, self.config, save_writer);
    }

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
