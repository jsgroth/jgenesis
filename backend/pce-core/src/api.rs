use crate::bus::Bus;
use crate::input::InputState;
use crate::memory::{HuCard, Memory};
use crate::video::VideoSubsystem;
use bincode::{Decode, Encode};
use huc6280_emu::Huc6280;
use jgenesis_common::frontend::{
    AudioOutput, EmulatorConfigTrait, EmulatorTrait, FiniteF64, InputPoller, PartialClone,
    RenderFrameOptions, Renderer, SaveWriter, TickEffect, TickResult,
};
use jgenesis_proc_macros::ConfigDisplay;
use pce_config::{PceButton, PceInputs};
use std::fmt::{Debug, Display};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct PceEmulatorConfig {
    pub placeholder: u8,
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
    memory: Memory,
    cartridge: HuCard,
    input_state: InputState,
    config: PceEmulatorConfig,
    cycle_counter: u64,
}

impl PcEngineEmulator {
    #[must_use]
    pub fn create(rom: Vec<u8>, config: PceEmulatorConfig) -> Self {
        let mut emulator = Self {
            cpu: Huc6280::new(),
            video: VideoSubsystem::new(),
            memory: Memory::new(),
            cartridge: HuCard::new(rom),
            input_state: InputState::new(),
            config,
            cycle_counter: 0,
        };

        emulator.cpu.reset(&mut Bus {
            memory: &mut emulator.memory,
            video: &mut emulator.video,
            cartridge: &emulator.cartridge,
            input: &mut emulator.input_state,
            cycle_counter: &mut emulator.cycle_counter,
        });

        emulator
    }

    fn render_frame<R: Renderer>(&mut self, renderer: &mut R) -> Result<(), R::Err> {
        self.video.render_rgba8_frame_buffer();

        renderer.render_frame(
            self.video.frame_buffer(),
            self.video.frame_size(),
            60.0, // TODO
            RenderFrameOptions {
                pixel_aspect_ratio: Some(FiniteF64::try_from(8.0 / 7.0 / 4.0).unwrap()),
                ..RenderFrameOptions::default()
            },
        )
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
            cartridge: &self.cartridge,
            input: &mut self.input_state,
            cycle_counter: &mut self.cycle_counter,
        });

        if self.video.frame_complete() {
            self.video.clear_frame_complete();

            self.render_frame(renderer).map_err(PceError::Render)?;

            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        todo!("force render")
    }

    fn reload_config(&mut self, config: &Self::Config) {
        todo!("reload config")
    }

    fn soft_reset(&mut self) {
        todo!("soft reset")
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        todo!("hard reset")
    }

    fn load_state(&mut self, state: Self::SaveState) {
        todo!("load state")
    }

    fn to_save_state(&self) -> Self::SaveState {
        self.partial_clone()
    }

    fn target_fps(&self) -> f64 {
        // TODO put actual value here
        60.0
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        // TODO once audio is implemented
    }
}
