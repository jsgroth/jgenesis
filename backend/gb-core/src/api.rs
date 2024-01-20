//! Game Boy emulator public interface and main loop

use crate::bus::Bus;
use crate::cartridge::Cartridge;
use crate::graphics::RgbaFrameBuffer;
use crate::inputs::{GameBoyInputs, InputState};
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu;
use crate::ppu::Ppu;
use crate::sm83::Sm83;
use crate::timer::GbTimer;
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, PixelAspectRatio, Renderer, SaveWriter, TickEffect, TickResult,
    TimingMode,
};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr, PartialClone};
use std::fmt::{Debug, Display};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GameBoyLoadError {
    #[error("ROM header contains invalid SRAM size byte: ${0:02X}")]
    InvalidSramByte(u8),
    #[error("ROM header contains unsupported mapper byte: ${0:02X}")]
    UnsupportedMapperByte(u8),
}

#[derive(Debug, Error)]
pub enum GameBoyError<RErr, AErr, SErr> {
    #[error("Error rendering a frame: {0}")]
    Rendering(RErr),
    #[error("Error outputting audio samples: {0}")]
    Audio(AErr),
    #[error("Error writing save file: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GbPalette {
    BlackAndWhite,
    #[default]
    GreenTint,
    LimeGreen,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct GameBoyEmulatorConfig {
    pub gb_palette: GbPalette,
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct GameBoyEmulator {
    cpu: Sm83,
    ppu: Ppu,
    memory: Memory,
    interrupt_registers: InterruptRegisters,
    #[partial_clone(partial)]
    cartridge: Cartridge,
    timer: GbTimer,
    input_state: InputState,
    rgba_buffer: RgbaFrameBuffer,
    config: GameBoyEmulatorConfig,
}

impl GameBoyEmulator {
    pub fn create(
        rom: Vec<u8>,
        initial_sram: Option<Vec<u8>>,
        config: GameBoyEmulatorConfig,
    ) -> Result<Self, GameBoyLoadError> {
        let cartridge = Cartridge::create(rom.into_boxed_slice(), initial_sram)?;

        Ok(Self {
            cpu: Sm83::new(),
            ppu: Ppu::new(),
            memory: Memory::new(),
            interrupt_registers: InterruptRegisters::default(),
            cartridge,
            timer: GbTimer::new(),
            input_state: InputState::new(),
            rgba_buffer: RgbaFrameBuffer::default(),
            config,
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
        self.input_state.set_inputs(*inputs);

        self.cpu.execute_instruction(&mut Bus {
            ppu: &mut self.ppu,
            memory: &mut self.memory,
            cartridge: &mut self.cartridge,
            interrupt_registers: &mut self.interrupt_registers,
            timer: &mut self.timer,
            input_state: &mut self.input_state,
        });

        if self.ppu.frame_complete() {
            self.ppu.clear_frame_complete();
            self.rgba_buffer.copy_from(self.ppu.frame_buffer(), self.config.gb_palette);
            renderer
                .render_frame(
                    self.rgba_buffer.as_ref(),
                    ppu::FRAME_SIZE,
                    Some(PixelAspectRatio::SQUARE),
                )
                .map_err(GameBoyError::Rendering)?;

            // TODO audio etc.

            Ok(TickEffect::FrameRendered)
        } else {
            Ok(TickEffect::None)
        }
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
        log::warn!("The Game Boy does not support soft reset except in software");
    }

    fn hard_reset(&mut self) {
        todo!("hard reset")
    }

    fn timing_mode(&self) -> TimingMode {
        TimingMode::Ntsc
    }
}
