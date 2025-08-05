//! GBA emulator public interface

use crate::apu::Apu;
use crate::bus::Bus;
use crate::cartridge::Cartridge;
use crate::dma::DmaState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu;
use crate::ppu::Ppu;
use crate::sio::SerialPort;
use crate::timers::Timers;
use arm7tdmi_emu::Arm7Tdmi;
use bincode::{Decode, Encode};
use gba_config::{GbaAspectRatio, GbaButton, GbaColorCorrection, GbaInputs};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorConfigTrait, EmulatorTrait, Renderer, SaveWriter, TickEffect, TickResult,
};
use jgenesis_proc_macros::{ConfigDisplay, PartialClone};
use std::fmt::{Debug, Display};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct GbaEmulatorConfig {
    pub aspect_ratio: GbaAspectRatio,
    pub color_correction: GbaColorCorrection,
}

#[derive(Debug, Error)]
pub enum GbaLoadError {
    #[error("Invalid BIOS ROM; expected length of {expected} bytes, was {actual} bytes")]
    InvalidBiosLength { expected: usize, actual: usize },
}

#[derive(Debug, Error)]
pub enum GbaError<RErr, AErr, SErr> {
    #[error("Error rendering video output: {0}")]
    Render(RErr),
    #[error("Error outputting audio samples: {0}")]
    Audio(AErr),
    #[error("Error persisting save file: {0}")]
    SaveWrite(SErr),
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub(crate) struct BusState {
    pub cycles: u64,
    pub cpu_pc: u32,
    pub last_bios_read: u32,
    pub open_bus: u32,
}

impl BusState {
    fn new() -> Self {
        Self { cycles: 0, cpu_pc: 0, last_bios_read: 0, open_bus: 0 }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct GameBoyAdvanceEmulator {
    cpu: Arm7Tdmi,
    #[partial_clone(partial)]
    bus: Bus,
    config: GbaEmulatorConfig,
}

impl GameBoyAdvanceEmulator {
    /// # Errors
    ///
    /// Returns an error if emulator initialization fails, e.g. because the BIOS ROM is invalid.
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        bios_rom: Vec<u8>,
        config: GbaEmulatorConfig,
        save_writer: &mut S,
    ) -> Result<Self, GbaLoadError> {
        let initial_save = save_writer.load_bytes("sav").ok();

        let ppu = Ppu::new(config);
        let apu = Apu::new();
        let memory = Memory::new(bios_rom)?;
        let cartridge = Cartridge::new(rom, initial_save);
        let dma = DmaState::new();
        let timers = Timers::new();
        let interrupts = InterruptRegisters::new();
        let sio = SerialPort::new();

        let mut cpu = Arm7Tdmi::new();
        let mut bus = Bus {
            ppu,
            apu,
            memory,
            cartridge,
            dma,
            timers,
            interrupts,
            sio,
            inputs: GbaInputs::default(),
            state: BusState::new(),
        };

        cpu.reset(&mut bus);

        Ok(Self { cpu, bus, config })
    }
}

impl EmulatorConfigTrait for GbaEmulatorConfig {}

impl EmulatorTrait for GameBoyAdvanceEmulator {
    type Button = GbaButton;
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
        self.bus.inputs = *inputs;

        if !self.bus.try_progress_dma() {
            self.cpu.execute_instruction(&mut self.bus);
        }

        self.bus.timers.step_to(
            self.bus.state.cycles,
            &mut self.bus.apu,
            &mut self.bus.dma,
            &mut self.bus.interrupts,
        );

        self.bus.apu.step_to(self.bus.state.cycles);
        self.bus.apu.drain_audio_output(audio_output).map_err(GbaError::Audio)?;

        self.bus.sync_ppu();
        if self.bus.ppu.frame_complete() {
            self.bus.ppu.clear_frame_complete();

            renderer
                .render_frame(
                    self.bus.ppu.frame_buffer(),
                    ppu::FRAME_SIZE,
                    self.config.aspect_ratio.to_pixel_aspect_ratio(),
                )
                .map_err(GbaError::Render)?;

            if self.bus.cartridge.take_rw_memory_dirty()
                && let Some(rw_memory) = self.bus.cartridge.rw_memory()
            {
                save_writer.persist_bytes("sav", rw_memory).map_err(GbaError::SaveWrite)?;
            }

            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        renderer.render_frame(
            self.bus.ppu.frame_buffer(),
            ppu::FRAME_SIZE,
            self.config.aspect_ratio.to_pixel_aspect_ratio(),
        )
    }

    fn reload_config(&mut self, config: &Self::Config) {
        self.bus.ppu.reload_config(*config);
        self.config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.bus.cartridge.take_rom_from(&mut other.bus.cartridge);
    }

    fn soft_reset(&mut self) {
        log::warn!("GBA does not support soft reset except in software");
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = self.bus.cartridge.take_rom();
        let bios_rom = self.bus.memory.clone_bios_rom();

        *self = Self::create(rom, bios_rom, self.config, save_writer)
            .expect("Emulator creation should never fail during hard reset");
    }

    fn target_fps(&self) -> f64 {
        // Roughly 59.73 fps
        (crate::GBA_CLOCK_SPEED as f64)
            / f64::from(ppu::LINES_PER_FRAME)
            / f64::from(ppu::DOTS_PER_LINE)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.bus.apu.update_output_frequency(output_frequency);
    }
}
