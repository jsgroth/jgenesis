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
    AudioOutput, Color, EmulatorTrait, PixelAspectRatio, Renderer, SaveWriter, TickEffect,
    TickResult, TimingMode,
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
pub enum GbAspectRatio {
    #[default]
    SquarePixels,
    Stretched,
}

impl GbAspectRatio {
    fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GbPalette {
    BlackAndWhite,
    #[default]
    GreenTint,
    LimeGreen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum GbcColorCorrection {
    None,
    #[default]
    GbcLcd,
    GbaLcd,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct GameBoyEmulatorConfig {
    pub force_dmg_mode: bool,
    pub pretend_to_be_gba: bool,
    pub aspect_ratio: GbAspectRatio,
    pub gb_palette: GbPalette,
    pub gbc_color_correction: GbcColorCorrection,
    pub audio_60hz_hack: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundTileMap {
    #[default]
    Zero,
    One,
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
    frame_count: u64,
}

impl GameBoyEmulator {
    /// # Errors
    ///
    /// This function will return an error if it cannot load the ROM (e.g. unsupported mapper).
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        config: GameBoyEmulatorConfig,
        save_writer: &mut S,
    ) -> Result<Self, GameBoyLoadError> {
        let software_type = SoftwareType::from_rom(&rom);

        let initial_sram = save_writer.load_bytes("sav").ok();
        let cartridge = Cartridge::create(rom.into_boxed_slice(), initial_sram, save_writer)?;

        let hardware_mode = match (config.force_dmg_mode, software_type) {
            (true, _) | (_, SoftwareType::DmgOnly) => HardwareMode::Dmg,
            (false, SoftwareType::CgbEnhanced | SoftwareType::CgbOnly) => HardwareMode::Cgb,
        };

        log::info!("Running with hardware mode {hardware_mode}");

        Ok(Self {
            hardware_mode,
            cpu: Sm83::new(hardware_mode, config.pretend_to_be_gba),
            ppu: Ppu::new(hardware_mode),
            apu: Apu::new(config),
            memory: Memory::new(),
            interrupt_registers: InterruptRegisters::default(),
            speed_register: SpeedRegister::new(),
            cartridge,
            timer: GbTimer::new(),
            dma_unit: DmaUnit::new(),
            input_state: InputState::new(),
            rgba_buffer: RgbaFrameBuffer::default(),
            config,
            frame_count: 0,
        })
    }

    pub fn copy_background(&self, tile_map: BackgroundTileMap, out: &mut [Color]) {
        self.ppu.copy_background(tile_map, out);
    }

    pub fn copy_sprites(&self, out: &mut [Color]) {
        self.ppu.copy_sprites(out);
    }

    pub fn copy_palettes(&self, out: &mut [Color]) {
        self.ppu.copy_palettes(out);
    }

    #[inline]
    #[must_use]
    pub fn is_using_double_height_sprites(&self) -> bool {
        self.ppu.is_using_double_height_sprites()
    }

    #[inline]
    #[must_use]
    pub fn is_cgb_mode(&self) -> bool {
        self.hardware_mode == HardwareMode::Cgb
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
                self.config.gbc_color_correction,
            );
            renderer
                .render_frame(
                    self.rgba_buffer.as_ref(),
                    ppu::FRAME_SIZE,
                    self.config.aspect_ratio.to_pixel_aspect_ratio(),
                )
                .map_err(GameBoyError::Rendering)?;

            self.apu.drain_samples_into(audio_output).map_err(GameBoyError::Audio)?;

            self.cartridge.update_rtc_time();

            if self.cartridge.has_battery()
                && self.frame_count % 60 == 30
                && self.cartridge.get_and_clear_sram_dirty()
            {
                let sram = self.cartridge.sram();
                save_writer.persist_bytes("sav", sram).map_err(GameBoyError::SaveWrite)?;

                self.cartridge.save_rtc_state(save_writer).map_err(GameBoyError::SaveWrite)?;
            }

            self.frame_count += 1;

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
            self.config.gbc_color_correction,
        );
        renderer.render_frame(
            self.rgba_buffer.as_ref(),
            ppu::FRAME_SIZE,
            self.config.aspect_ratio.to_pixel_aspect_ratio(),
        )
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.config = *config;
        self.apu.reload_config(*config);
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.cartridge.take_rom_from(&mut other.cartridge);
    }

    fn soft_reset(&mut self) {
        log::warn!("The Game Boy does not support soft reset except in software");
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = self.cartridge.take_rom();

        *self = Self::create(rom, self.config, save_writer)
            .expect("Hard reset should never fail to load cartridge");
    }

    fn timing_mode(&self) -> TimingMode {
        TimingMode::Ntsc
    }
}
