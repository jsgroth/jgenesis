use crate::input::GbaInputs;
use arm7tdmi_emu::Arm7Tdmi;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, PartialClone, Renderer, SaveWriter, TickResult, TimingMode,
};
use std::fmt::{Debug, Display};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct GbaEmulatorConfig {}

#[derive(Debug, Error)]
pub enum GbaError<RErr, AErr, SErr> {
    #[error("Error rendering frame: {0}")]
    Render(RErr),
    #[error("Error playing audio samples: {0}")]
    Audio(AErr),
    #[error("Error writing save file: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct GameBoyAdvanceEmulator {
    cpu: Arm7Tdmi,
}

impl GameBoyAdvanceEmulator {
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        config: GbaEmulatorConfig,
        save_writer: &mut S,
    ) -> Self {
        Self { cpu: Arm7Tdmi::new() }
    }
}

impl EmulatorTrait for GameBoyAdvanceEmulator {
    type Inputs = GbaInputs;
    type Config = GbaEmulatorConfig;
    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = GbaError<RErr, AErr, SErr>;

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
        todo!("tick")
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

    fn take_rom_from(&mut self, other: &mut Self) {
        todo!("take ROM from")
    }

    fn soft_reset(&mut self) {
        todo!("soft reset")
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        todo!("hard reset")
    }

    fn timing_mode(&self) -> TimingMode {
        TimingMode::Ntsc
    }

    fn target_fps(&self) -> f64 {
        // TODO figure out actual refresh rate
        60.0
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        log::error!(
            "Ignoring audio output frequency update to {output_frequency}; audio resampling not yet implemented"
        );
    }
}
