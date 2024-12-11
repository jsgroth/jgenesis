//! GBA public interface and main loop

use crate::apu::Apu;
use crate::bus::Bus;
use crate::cartridge::Cartridge;
use crate::control::ControlRegisters;
use crate::input::GbaInputs;
use crate::memory::Memory;
use crate::ppu::{Ppu, PpuTickEffect};
use crate::timers::Timers;
use crate::{bus, ppu};
use arm7tdmi_emu::{Arm7Tdmi, Arm7TdmiResetArgs, CpuMode};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, PartialClone, PixelAspectRatio, Renderer, SaveWriter, TickEffect,
    TickResult, TimingMode,
};
use jgenesis_proc_macros::{EnumAll, EnumDisplay};
use std::fmt::{Debug, Display};
use std::mem;
use thiserror::Error;

// 1 PPU cycle per 4 CPU cycles
const PPU_DIVIDER: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumAll, EnumDisplay)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum GbaAspectRatio {
    #[default]
    SquarePixels,
    Stretched,
}

impl GbaAspectRatio {
    fn to_pixel_aspect_ratio(self) -> Option<PixelAspectRatio> {
        match self {
            Self::SquarePixels => Some(PixelAspectRatio::SQUARE),
            Self::Stretched => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct GbaEmulatorConfig {
    pub aspect_ratio: GbaAspectRatio,
    pub skip_bios_intro_animation: bool,
}

#[derive(Debug, Error)]
pub enum GbaError<RErr, AErr, SErr> {
    #[error("Error rendering frame: {0}")]
    Render(RErr),
    #[error("Error playing audio samples: {0}")]
    Audio(AErr),
    #[error("Error writing save file: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, Error)]
pub enum GbaInitializationError {
    #[error("Invalid BIOS ROM; expected length 16384 bytes, was {length} bytes")]
    InvalidBiosRom { length: usize },
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct GameBoyAdvanceEmulator {
    cpu: Arm7Tdmi,
    ppu: Ppu,
    apu: Apu,
    #[partial_clone(partial)]
    memory: Memory,
    control: ControlRegisters,
    timers: Timers,
    ppu_mclk_counter: u32,
    config: GbaEmulatorConfig,
}

macro_rules! new_bus {
    ($self:expr, $inputs:expr) => {
        Bus {
            ppu: &mut $self.ppu,
            apu: &mut $self.apu,
            memory: &mut $self.memory,
            control: &mut $self.control,
            timers: &mut $self.timers,
            inputs: $inputs,
        }
    };
}

impl GameBoyAdvanceEmulator {
    /// # Errors
    ///
    /// Returns an error if the BIOS ROM is not the expected length (16KB).
    pub fn create<S: SaveWriter>(
        cartridge_rom: Vec<u8>,
        bios_rom: Vec<u8>,
        config: GbaEmulatorConfig,
        save_writer: &mut S,
    ) -> Result<Self, GbaInitializationError> {
        let cartridge = Cartridge::new(cartridge_rom);
        let memory = Memory::new(cartridge, bios_rom)?;

        let mut emulator = Self {
            cpu: Arm7Tdmi::new(),
            ppu: Ppu::new(),
            apu: Apu::new(),
            memory,
            control: ControlRegisters::new(),
            timers: Timers::new(),
            ppu_mclk_counter: 0,
            config,
        };

        let mut bus = new_bus!(emulator, GbaInputs::default());
        if config.skip_bios_intro_animation {
            emulator.cpu.manual_reset(
                Arm7TdmiResetArgs {
                    pc: bus::CARTRIDGE_ROM_0_START,
                    sp_usr: 0x03007F00,
                    sp_svc: 0x03007FE0,
                    sp_irq: 0x03007FA0,
                    sp_fiq: 0x00000000,
                    mode: CpuMode::System,
                },
                &mut bus,
            );
            bus.control.postflg = 1;
        } else {
            emulator.cpu.reset(&mut bus);
        }

        Ok(emulator)
    }

    fn render_frame<R: Renderer>(&self, renderer: &mut R) -> Result<(), R::Err> {
        renderer.render_frame(
            self.ppu.frame_buffer(),
            ppu::FRAME_SIZE,
            self.config.aspect_ratio.to_pixel_aspect_ratio(),
        )
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
        let mut bus = new_bus!(self, *inputs);

        let cpu_cycles = match bus.control.dma_state.active_channels.first().copied() {
            Some(channel_idx) => {
                let mut channel = bus.control.dma[channel_idx as usize].clone();
                let cycles = channel.run_dma(&mut bus);
                bus.control.dma[channel_idx as usize] = channel;
                cycles
            }
            None => self.cpu.execute_instruction(&mut bus),
        };

        self.timers.tick(cpu_cycles, &mut self.apu, &mut self.control);
        self.apu.tick(cpu_cycles, audio_output).map_err(GbaError::Audio)?;
        self.control.update_audio_drq(&self.apu);

        self.ppu_mclk_counter += cpu_cycles;
        let ppu_cycles = self.ppu_mclk_counter / PPU_DIVIDER;
        self.ppu_mclk_counter -= ppu_cycles * PPU_DIVIDER;

        if self.ppu.tick(ppu_cycles, &mut self.control) == PpuTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(GbaError::Render)?;
            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        self.render_frame(renderer)
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.cartridge.rom = mem::take(&mut other.memory.cartridge.rom);
    }

    fn soft_reset(&mut self) {
        log::warn!("Game Boy Advance does not support soft reset at the hardware level");
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let cartridge_rom = mem::take(&mut self.memory.cartridge.rom.0);
        let bios_rom = self.memory.bios.clone();
        *self =
            Self::create(cartridge_rom.into_vec(), bios_rom.into_vec(), self.config, save_writer)
                .expect("Creating a new emulator instance during hard reset should never fail");
    }

    fn timing_mode(&self) -> TimingMode {
        TimingMode::Ntsc
    }

    fn target_fps(&self) -> f64 {
        // ~59.73 Hz, same as GB/GBC
        f64::from(crate::GBA_CLOCK_RATE)
            / f64::from(PPU_DIVIDER)
            / f64::from(ppu::LINES_PER_FRAME)
            / f64::from(ppu::DOTS_PER_LINE)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.apu.update_output_frequency(output_frequency);
    }
}
