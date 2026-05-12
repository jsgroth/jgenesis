use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::{HuCard, Memory};
use crate::psg::Huc6280Psg;
use crate::video;
use crate::video::VideoSubsystem;
use bincode::{Decode, Encode};
use huc6280_emu::Huc6280;
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorConfigTrait, EmulatorTrait, FiniteF64, InputPoller, PartialClone,
    RenderFrameOptions, Renderer, SaveWriter, TickEffect, TickResult,
};
use jgenesis_proc_macros::ConfigDisplay;
use pce_config::{PceAspectRatio, PceButton, PceInputs, PceRegion};
use std::fmt::{Debug, Display};
use thiserror::Error;

// Roughly 21.47 MHz
pub const MASTER_CLOCK_FREQUENCY: f64 = 236.25e6 / 11.0;

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct PceEmulatorConfig {
    pub region: PceRegion,
    pub aspect_ratio: PceAspectRatio,
    pub crop_overscan: bool,
    pub remove_sprite_limits: bool,
}

impl EmulatorConfigTrait for PceEmulatorConfig {}

#[derive(Debug, Error)]
pub enum PceError<RErr, AErr, SErr> {
    #[error("Error rendering frame: {0}")]
    Render(RErr),
    #[error("Error outputting audio: {0}")]
    Audio(AErr),
    #[error("Error writing save file: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, Clone, PartialClone, Encode, Decode)]
pub struct PcEngineEmulator {
    cpu: Huc6280,
    video: VideoSubsystem,
    psg: Huc6280Psg,
    memory: Memory,
    cartridge: HuCard,
    input_state: InputState,
    config: PceEmulatorConfig,
    cycle_counter: u64,
    last_psg_sync_cycles: u64,
}

impl PcEngineEmulator {
    #[must_use]
    pub fn create(rom: Vec<u8>, config: PceEmulatorConfig) -> Self {
        let mut emulator = Self {
            cpu: Huc6280::new(),
            video: VideoSubsystem::new(config),
            psg: Huc6280Psg::new(),
            memory: Memory::new(),
            cartridge: HuCard::new(rom),
            input_state: InputState::new(config),
            config,
            cycle_counter: 0,
            last_psg_sync_cycles: 0,
        };

        emulator.cpu.reset(&mut Bus {
            memory: &mut emulator.memory,
            video: &mut emulator.video,
            psg: &mut emulator.psg,
            cartridge: &emulator.cartridge,
            input: &mut emulator.input_state,
            cycle_counter: &mut emulator.cycle_counter,
        });

        emulator
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        self.video.render_rgba8_frame_buffer();

        let aspect_ratio = match self.config.aspect_ratio {
            // TODO vary based on H resolution
            PceAspectRatio::Ntsc => Some(FiniteF64::try_from(8.0 / 7.0 / 4.0).unwrap()),
            PceAspectRatio::SquarePixels => Some(FiniteF64::try_from(1.0 / 4.0).unwrap()),
            PceAspectRatio::Stretched => None,
        };

        renderer.render_frame(
            self.video.frame_buffer(),
            self.video.frame_size(),
            self.video.target_fps(),
            RenderFrameOptions {
                pixel_aspect_ratio: aspect_ratio,
                ..RenderFrameOptions::default()
            },
        )
    }

    pub fn dump_vram(&self, palette: u16, out: &mut [[Color; 64]]) {
        self.video.dump_vram(palette, out);
    }

    pub fn dump_palettes(&self, out: &mut [Color]) {
        self.video.dump_palettes(out);
    }
}

impl EmulatorTrait for PcEngineEmulator {
    type Button = PceButton;
    type Inputs = PceInputs;
    type Config = PceEmulatorConfig;
    type SaveState = Self;

    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = PceError<RErr, AErr, SErr>;

    fn tick<R, A, I, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        input_poller: &mut I,
        save_writer: &mut S,
    ) -> TickResult<Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        A: AudioOutput,
        I: InputPoller<Self::Inputs>,
        S: SaveWriter,
    {
        self.input_state.update_inputs(*input_poller.poll());

        self.cpu.execute_instruction(&mut Bus {
            memory: &mut self.memory,
            video: &mut self.video,
            psg: &mut self.psg,
            cartridge: &self.cartridge,
            input: &mut self.input_state,
            cycle_counter: &mut self.cycle_counter,
        });

        // Sync PSG here in case the VDC blocked a CPU VRAM access for a long amount of time across
        // a frame boundary
        if self.cycle_counter - self.last_psg_sync_cycles >= video::MCLK_CYCLES_PER_SCANLINE {
            self.psg.step_to(self.cycle_counter);
            self.psg.drain_output_buffer(audio_output).map_err(PceError::Audio)?;

            self.last_psg_sync_cycles = self.cycle_counter;
        }

        if self.video.frame_complete() {
            self.video.clear_frame_complete();

            self.render_frame(renderer).map_err(PceError::Render)?;

            Ok(TickEffect::FrameRendered)
        } else {
            Ok(TickEffect::None)
        }
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.config = *config;

        self.video.reload_config(*config);
        self.input_state.reload_config(*config);
    }

    fn soft_reset(&mut self) {
        log::warn!("PC Engine does not support soft reset except in software");
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = self.cartridge.clone_rom();
        *self = Self::create(rom, self.config);
    }

    fn load_state(&mut self, mut state: Self::SaveState) {
        state.cartridge.take_rom_from(&mut self.cartridge);
        *self = state;
    }

    fn to_save_state(&self) -> Self::SaveState {
        self.partial_clone()
    }

    fn target_fps(&self) -> f64 {
        self.video.target_fps()
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.psg.update_output_frequency(output_frequency);
    }
}
