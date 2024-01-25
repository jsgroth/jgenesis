//! Game Boy emulator public interface and main loop

use crate::apu::Apu;
use crate::bus::Bus;
use crate::cartridge::{Cartridge, SoftwareType};
use crate::dma::DmaUnit;
use crate::graphics::RgbaFrameBuffer;
use crate::inputs::{GameBoyInputs, InputState};
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::sm83::Sm83;
use crate::speed::SpeedRegister;
use crate::timer::GbTimer;
use crate::{ppu, HardwareMode};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, PixelAspectRatio, Renderer, SaveWriter, TickEffect, TickResult,
    TimingMode,
};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr, PartialClone};
use std::fmt::{Debug, Display};
use std::iter;
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
    pub force_dmg_mode: bool,
    pub gb_palette: GbPalette,
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct GameBoyEmulator {
    hardware_mode: HardwareMode,
    cpu: Sm83,
    ppu: Ppu,
    apu: Apu,
    memory: Memory,
    interrupt_registers: InterruptRegisters,
    speed_register: SpeedRegister,
    #[partial_clone(partial)]
    cartridge: Cartridge,
    timer: GbTimer,
    dma_unit: DmaUnit,
    input_state: InputState,
    rgba_buffer: RgbaFrameBuffer,
    config: GameBoyEmulatorConfig,
}

impl GameBoyEmulator {
    /// # Errors
    ///
    /// This function will return an error if it cannot load the ROM (e.g. unsupported mapper).
    pub fn create(
        rom: Vec<u8>,
        initial_sram: Option<Vec<u8>>,
        config: GameBoyEmulatorConfig,
    ) -> Result<Self, GameBoyLoadError> {
        let software_type = SoftwareType::from_rom(&rom);
        let cartridge = Cartridge::create(rom.into_boxed_slice(), initial_sram)?;

        let hardware_mode = match (config.force_dmg_mode, software_type) {
            (true, _) | (_, SoftwareType::DmgOnly) => HardwareMode::Dmg,
            (false, SoftwareType::CgbEnhanced | SoftwareType::CgbOnly) => HardwareMode::Cgb,
        };

        log::info!("Running with hardware mode {hardware_mode}");

        Ok(Self {
            hardware_mode,
            cpu: Sm83::new(hardware_mode),
            ppu: Ppu::new(hardware_mode),
            apu: Apu::new(),
            memory: Memory::new(),
            interrupt_registers: InterruptRegisters::default(),
            speed_register: SpeedRegister::new(),
            cartridge,
            timer: GbTimer::new(),
            dma_unit: DmaUnit::new(),
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
            hardware_mode: self.hardware_mode,
            ppu: &mut self.ppu,
            apu: &mut self.apu,
            memory: &mut self.memory,
            cartridge: &mut self.cartridge,
            interrupt_registers: &mut self.interrupt_registers,
            speed_register: &mut self.speed_register,
            timer: &mut self.timer,
            dma_unit: &mut self.dma_unit,
            input_state: &mut self.input_state,
        });

        if self.ppu.frame_complete() {
            self.ppu.clear_frame_complete();
            self.rgba_buffer.copy_from(
                self.ppu.frame_buffer(),
                self.hardware_mode,
                self.config.gb_palette,
            );
            renderer
                .render_frame(
                    self.rgba_buffer.as_ref(),
                    ppu::FRAME_SIZE,
                    Some(PixelAspectRatio::SQUARE),
                )
                .map_err(GameBoyError::Rendering)?;

            self.apu.drain_samples_into(audio_output).map_err(GameBoyError::Audio)?;

            let sram = self.cartridge.sram();
            if !sram.is_empty() {
                save_writer.persist_save(iter::once(sram)).map_err(GameBoyError::SaveWrite)?;
            }

            Ok(TickEffect::FrameRendered)
        } else if self.apu.queued_sample_count() > 1200 {
            // A frame and a half's worth of samples are queued up; this can happen when the PPU is disabled
            // Push the samples and pretend to render a frame so that the frontend will process events
            self.apu.drain_samples_into(audio_output).map_err(GameBoyError::Audio)?;

            Ok(TickEffect::FrameRendered)
        } else {
            Ok(TickEffect::None)
        }
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.rgba_buffer.copy_from(
            self.ppu.frame_buffer(),
            self.hardware_mode,
            self.config.gb_palette,
        );
        renderer.render_frame(
            self.rgba_buffer.as_ref(),
            ppu::FRAME_SIZE,
            Some(PixelAspectRatio::SQUARE),
        )
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.cartridge.take_rom_from(&mut other.cartridge);
    }

    fn soft_reset(&mut self) {
        log::warn!("The Game Boy does not support soft reset except in software");
    }

    fn hard_reset(&mut self) {
        let rom = self.cartridge.take_rom();
        let sram = self.cartridge.sram().to_vec();

        *self = Self::create(rom, Some(sram), self.config)
            .expect("Hard reset should never fail to load cartridge");
    }

    fn timing_mode(&self) -> TimingMode {
        TimingMode::Ntsc
    }
}
