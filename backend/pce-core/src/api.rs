use bincode::{Decode, Encode};
use huc6280_emu::Huc6280;
use jgenesis_common::frontend::{
    AudioOutput, EmulatorConfigTrait, EmulatorTrait, InputPoller, PartialClone, Renderer,
    SaveWriter, TickResult,
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
    rom: Vec<u8>,
    config: PceEmulatorConfig,
}

impl PcEngineEmulator {
    pub fn create(rom: Vec<u8>, config: PceEmulatorConfig) -> Self {
        Self { cpu: Huc6280::new(), rom, config }
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
