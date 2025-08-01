//! GBA emulator public interface

use crate::apu::Apu;
use crate::bus::Bus;
use crate::cartridge::Cartridge;
use crate::dma::DmaState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu;
use crate::ppu::Ppu;
use crate::timers::Timers;
use arm7tdmi_emu::{Arm7Tdmi, Arm7TdmiResetArgs, CpuMode};
use bincode::{Decode, Encode};
use gba_config::{GbaButton, GbaInputs};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorConfigTrait, EmulatorTrait, PixelAspectRatio, Renderer, SaveWriter,
    TickEffect, TickResult,
};
use jgenesis_proc_macros::PartialClone;
use std::fmt::{Debug, Display, Formatter};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct GbaEmulatorConfig;

impl Display for GbaEmulatorConfig {
    fn fmt(&self, _f: &mut Formatter<'_>) -> std::fmt::Result {
        Ok(())
    }
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
}

impl BusState {
    fn new() -> Self {
        Self { cycles: 0, cpu_pc: 0, last_bios_read: 0 }
    }
}

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct GameBoyAdvanceEmulator {
    cpu: Arm7Tdmi,
    ppu: Ppu,
    apu: Apu,
    memory: Memory,
    #[partial_clone(partial)]
    cartridge: Cartridge,
    dma: DmaState,
    timers: Timers,
    interrupts: InterruptRegisters,
    bus_state: BusState,
    config: GbaEmulatorConfig,
}

impl GameBoyAdvanceEmulator {
    /// # Errors
    ///
    /// Returns an error if emulator initialization fails, e.g. because the BIOS ROM is invalid.
    pub fn create(
        rom: Vec<u8>,
        bios_rom: Vec<u8>,
        config: GbaEmulatorConfig,
    ) -> Result<Self, GbaLoadError> {
        let mut ppu = Ppu::new();
        let mut apu = Apu::new();
        let mut memory = Memory::new(bios_rom)?;
        let mut cartridge = Cartridge::new(rom);
        let mut dma = DmaState::new();
        let mut timers = Timers::new();
        let mut interrupts = InterruptRegisters::new();

        // TODO BIOS boot
        let mut cpu = Arm7Tdmi::new();
        cpu.manual_reset(
            Arm7TdmiResetArgs {
                pc: 0x8000000,
                sp_usr: 0x3007FF0,
                sp_svc: 0x3007FE0,
                sp_irq: 0x3007FA0,
                sp_fiq: 0x3007F60,
                mode: CpuMode::System,
            },
            &mut Bus {
                ppu: &mut ppu,
                apu: &mut apu,
                memory: &mut memory,
                cartridge: &mut cartridge,
                dma: &mut dma,
                timers: &mut timers,
                interrupts: &mut interrupts,
                inputs: &GbaInputs::default(),
                state: BusState::new(),
            },
        );

        Ok(Self {
            cpu,
            ppu,
            apu,
            memory,
            cartridge,
            dma,
            timers,
            interrupts,
            bus_state: BusState::new(),
            config,
        })
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
        let mut bus = Bus {
            ppu: &mut self.ppu,
            apu: &mut self.apu,
            memory: &mut self.memory,
            cartridge: &mut self.cartridge,
            dma: &mut self.dma,
            timers: &mut self.timers,
            interrupts: &mut self.interrupts,
            inputs,
            state: self.bus_state,
        };

        if !bus.try_progress_dma() {
            self.cpu.execute_instruction(&mut bus);
        }

        self.bus_state = bus.state;

        self.timers.step_to(
            self.bus_state.cycles,
            &mut self.apu,
            &mut self.dma,
            &mut self.interrupts,
        );

        self.apu.step_to(self.bus_state.cycles);
        self.apu.drain_audio_output(audio_output).map_err(GbaError::Audio)?;

        self.ppu.step_to(self.bus_state.cycles, &mut self.interrupts, &mut self.dma);
        if self.ppu.frame_complete() {
            self.ppu.clear_frame_complete();

            renderer
                .render_frame(
                    self.ppu.frame_buffer(),
                    ppu::FRAME_SIZE,
                    Some(PixelAspectRatio::SQUARE),
                )
                .map_err(GbaError::Render)?;

            return Ok(TickEffect::FrameRendered);
        }

        Ok(TickEffect::None)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        renderer.render_frame(
            self.ppu.frame_buffer(),
            ppu::FRAME_SIZE,
            Some(PixelAspectRatio::SQUARE),
        )
    }

    fn reload_config(&mut self, config: &Self::Config) {
        // TODO reload when there is config to reload
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.cartridge.take_rom_from(&mut other.cartridge);
    }

    fn soft_reset(&mut self) {
        log::warn!("GBA does not support soft reset except in software");
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let rom = self.cartridge.take_rom();
        let bios_rom = self.memory.clone_bios_rom();

        *self = Self::create(rom, bios_rom, self.config)
            .expect("Emulator creation should never fail during hard reset");
    }

    fn target_fps(&self) -> f64 {
        // Roughly 59.73 fps
        (crate::GBA_CLOCK_SPEED as f64)
            / f64::from(ppu::LINES_PER_FRAME)
            / f64::from(ppu::DOTS_PER_LINE)
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.apu.update_output_frequency(output_frequency);
    }
}
