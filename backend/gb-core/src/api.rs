//! Game Boy emulator public interface and main loop

use crate::bus::Bus;
use crate::cartridge::Cartridge;
use crate::inputs::GameBoyInputs;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::sm83::Sm83;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, Renderer, SaveWriter, TickEffect, TickResult, TimingMode,
};
use jgenesis_proc_macros::PartialClone;
use std::fmt::{Debug, Display};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GameBoyLoadError {}

#[derive(Debug, Error)]
pub enum GameBoyError<RErr, AErr, SErr> {
    #[error("Error rendering a frame: {0}")]
    Rendering(RErr),
    #[error("Error outputting audio samples: {0}")]
    Audio(AErr),
    #[error("Error writing save file: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct GameBoyEmulatorConfig {}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct GameBoyEmulator {
    cpu: Sm83,
    memory: Memory,
    interrupt_registers: InterruptRegisters,
    #[partial_clone(partial)]
    cartridge: Cartridge,
}

impl GameBoyEmulator {
    pub fn create(rom: Vec<u8>, initial_sram: Option<Vec<u8>>) -> Result<Self, GameBoyLoadError> {
        let cartridge = Cartridge::create(rom.into_boxed_slice())?;

        Ok(Self {
            cpu: Sm83::new(),
            memory: Memory::new(),
            interrupt_registers: InterruptRegisters::default(),
            cartridge,
        })
    }
}

impl EmulatorTrait for GameBoyEmulator {
    type Inputs = GameBoyInputs;
    type Config = GameBoyEmulatorConfig;
    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = GameBoyError<RErr, AErr, SErr>;

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
        self.cpu.execute_instruction(&mut Bus {
            memory: &mut self.memory,
            cartridge: &mut self.cartridge,
            interrupt_registers: &mut self.interrupt_registers,
        });

        // TODO check if PPU frame complete

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

    fn take_rom_from(&mut self, other: &mut Self) {
        todo!("take ROM from other")
    }

    fn soft_reset(&mut self) {
        todo!("soft reset")
    }

    fn hard_reset(&mut self) {
        todo!("hard reset")
    }

    fn timing_mode(&self) -> TimingMode {
        TimingMode::Ntsc
    }
}
