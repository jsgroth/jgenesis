//! Game Boy emulator public interface and main loop

use crate::apu::Apu;
use crate::bus::Bus;
use crate::cartridge::{Cartridge, SoftwareType};
use crate::cgb::CgbRegisters;
use crate::dma::DmaUnit;
use crate::graphics::RgbaFrameBuffer;
use crate::inputs::InputState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu::Ppu;
use crate::serial::SerialPort;
use crate::sm83::Sm83;
use crate::timer::GbTimer;
use crate::{HardwareMode, audio, ppu};
use bincode::{Decode, Encode};
use gb_config::{
    GameBoyButton, GameBoyInputs, GbAspectRatio, GbAudioResampler, GbPalette, GbcColorCorrection,
};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorConfigTrait, EmulatorTrait, Renderer, SaveWriter, TickEffect,
    TickResult,
};
use jgenesis_proc_macros::{ConfigDisplay, PartialClone};
use std::fmt::{Debug, Display};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GameBoyLoadError {
    #[error("ROM header contains invalid SRAM size byte: ${0:02X}")]
    InvalidSramByte(u8),
    #[error("ROM header contains unsupported mapper byte: ${0:02X}")]
    UnsupportedMapperByte(u8),
    #[error("Incorrect boot ROM size; expected {expected} bytes, was {actual} bytes")]
    InvalidBootRomSize { actual: usize, expected: usize },
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

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct GameBoyEmulatorConfig {
    pub force_dmg_mode: bool,
    pub force_cgb_mode: bool,
    pub pretend_to_be_gba: bool,
    pub aspect_ratio: GbAspectRatio,
    pub gb_palette: GbPalette,
    #[cfg_display(debug_fmt)]
    pub gb_custom_palette: [(u8, u8, u8); 4],
    pub gbc_color_correction: GbcColorCorrection,
    pub frame_blending: bool,
    pub audio_resampler: GbAudioResampler,
    pub audio_60hz_hack: bool,
}

impl EmulatorConfigTrait for GameBoyEmulatorConfig {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BackgroundTileMap {
    #[default]
    Zero,
    One,
}

pub struct BootRoms {
    pub dmg: Option<Vec<u8>>,
    pub cgb: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct GameBoyEmulator {
    hardware_mode: HardwareMode,
    cpu: Sm83,
    ppu: Ppu,
    apu: Apu,
    memory: Memory,
    serial_port: SerialPort,
    interrupt_registers: InterruptRegisters,
    cgb_registers: CgbRegisters,
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
        mut rom: Vec<u8>,
        boot_roms: BootRoms,
        config: GameBoyEmulatorConfig,
        save_writer: &mut S,
    ) -> Result<Self, GameBoyLoadError> {
        let software_type = SoftwareType::from_rom(&rom);
        let hardware_mode = match software_type {
            SoftwareType::DmgOnly => {
                if config.force_cgb_mode {
                    HardwareMode::Cgb
                } else {
                    HardwareMode::Dmg
                }
            }
            SoftwareType::CgbEnhanced | SoftwareType::CgbOnly => {
                if config.force_dmg_mode {
                    HardwareMode::Dmg
                } else {
                    HardwareMode::Cgb
                }
            }
        };

        log::info!("Running with hardware mode {hardware_mode}, software type {software_type}");

        let boot_rom = match hardware_mode {
            HardwareMode::Dmg => boot_roms.dmg,
            HardwareMode::Cgb => boot_roms.cgb,
        };
        let boot_rom_present = boot_rom.is_some();

        let ppu = Ppu::new(hardware_mode, &rom, boot_rom_present);
        let memory = Memory::new(boot_rom, hardware_mode)?;

        let initial_sram = save_writer.load_bytes("sav").ok();

        jgenesis_common::rom::mirror_to_next_power_of_two(&mut rom);
        let cartridge = Cartridge::create(rom.into_boxed_slice(), initial_sram, save_writer)?;

        Ok(Self {
            hardware_mode,
            cpu: Sm83::new(hardware_mode, config.pretend_to_be_gba, boot_rom_present),
            ppu,
            apu: Apu::new(config, hardware_mode),
            memory,
            serial_port: SerialPort::new(hardware_mode),
            interrupt_registers: InterruptRegisters::default(),
            cgb_registers: CgbRegisters::new(),
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
        self.ppu.copy_background(tile_map, self.cgb_registers.dmg_compatibility, out);
    }

    pub fn copy_sprites(&self, out: &mut [Color]) {
        self.ppu.copy_sprites(self.cgb_registers.dmg_compatibility, out);
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
    type Button = GameBoyButton;
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
            serial_port: &mut self.serial_port,
            cartridge: &mut self.cartridge,
            interrupt_registers: &mut self.interrupt_registers,
            cgb_registers: &mut self.cgb_registers,
            timer: &mut self.timer,
            dma_unit: &mut self.dma_unit,
            input_state: &mut self.input_state,
        });

        self.apu.drain_samples_into(audio_output).map_err(GameBoyError::Audio)?;

        self.input_state.check_for_joypad_interrupt(&mut self.interrupt_registers);

        if self.ppu.frame_complete() {
            self.ppu.clear_frame_complete();
            self.rgba_buffer.copy_from(self.ppu.frame_buffers(), self.hardware_mode, &self.config);
            renderer
                .render_frame(
                    self.rgba_buffer.as_ref(),
                    ppu::FRAME_SIZE,
                    self.config.aspect_ratio.to_pixel_aspect_ratio(),
                )
                .map_err(GameBoyError::Rendering)?;

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
        } else {
            Ok(TickEffect::None)
        }
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.rgba_buffer.copy_from(self.ppu.frame_buffers(), self.hardware_mode, &self.config);
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

        let boot_rom = self.memory.clone_boot_rom();
        let boot_roms = match self.hardware_mode {
            HardwareMode::Dmg => BootRoms { dmg: boot_rom, cgb: None },
            HardwareMode::Cgb => BootRoms { dmg: None, cgb: boot_rom },
        };

        *self = Self::create(rom, boot_roms, self.config, save_writer)
            .expect("Hard reset should never fail to load cartridge");
    }

    fn save_state_version() -> &'static str {
        "0.10.1-1"
    }

    fn target_fps(&self) -> f64 {
        if self.config.audio_60hz_hack {
            60.0
        } else {
            // Approximately 59.73 Hz
            let dots_per_frame = f64::from(ppu::DOTS_PER_LINE) * f64::from(ppu::LINES_PER_FRAME);
            4.0 * audio::GB_APU_FREQUENCY / dots_per_frame
        }
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.apu.update_output_frequency(output_frequency);
    }
}
