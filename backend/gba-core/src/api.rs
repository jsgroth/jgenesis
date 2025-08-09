//! GBA emulator public interface

use crate::apu::Apu;
use crate::bus::{Bus, BusState};
use crate::cartridge::Cartridge;
use crate::dma::DmaState;
use crate::input::InputState;
use crate::interrupts::InterruptRegisters;
use crate::memory::Memory;
use crate::ppu;
use crate::ppu::Ppu;
use crate::prefetch::GamePakPrefetcher;
use crate::sio::SerialPort;
use crate::timers::Timers;
use arm7tdmi_emu::bus::BusInterface;
use arm7tdmi_emu::{Arm7Tdmi, Arm7TdmiResetArgs, CpuMode};
use bincode::{Decode, Encode};
use gba_config::{GbaAspectRatio, GbaButton, GbaColorCorrection, GbaInputs, GbaSaveMemory};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorConfigTrait, EmulatorTrait, Renderer, SaveWriter, TickEffect, TickResult,
};
use jgenesis_proc_macros::{ConfigDisplay, PartialClone};
use std::fmt::{Debug, Display};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct GbaEmulatorConfig {
    pub skip_bios_animation: bool,
    pub aspect_ratio: GbaAspectRatio,
    pub color_correction: GbaColorCorrection,
    pub forced_save_memory_type: Option<GbaSaveMemory>,
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

#[derive(Debug, PartialClone, Encode, Decode)]
pub struct GameBoyAdvanceEmulator {
    cpu: Arm7Tdmi,
    #[partial_clone(partial)]
    bus: Bus,
    config: GbaEmulatorConfig,
    last_apu_sync_cycles: u64,
    frame_count: u64,
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
        let initial_rtc = save_writer.load_serialized("rtc").ok();

        let memory = Memory::new(bios_rom, config)?;
        let cartridge =
            Cartridge::new(rom, initial_save, initial_rtc, config.forced_save_memory_type);

        let mut cpu = Arm7Tdmi::new();
        let mut bus = Bus {
            ppu: Ppu::new(config),
            apu: Apu::new(),
            memory,
            cartridge,
            prefetch: GamePakPrefetcher::new(),
            dma: DmaState::new(),
            timers: Timers::new(),
            interrupts: InterruptRegisters::new(),
            sio: SerialPort::new(),
            inputs: InputState::new(),
            state: BusState::new(),
        };

        if !config.skip_bios_animation {
            cpu.reset(&mut bus);
        } else {
            cpu.manual_reset(
                Arm7TdmiResetArgs {
                    pc: 0x8000000,
                    sp_usr: 0x3007F00,
                    sp_svc: 0x3007FE0,
                    sp_irq: 0x3007FA0,
                    sp_fiq: 0,
                    mode: CpuMode::System,
                },
                &mut bus,
            );
        }

        Ok(Self { cpu, bus, config, last_apu_sync_cycles: 0, frame_count: 0 })
    }

    fn drain_apu<A: AudioOutput>(&mut self, audio_output: &mut A) -> Result<(), A::Err> {
        self.bus.apu.step_to(self.bus.state.cycles);
        self.bus.apu.drain_audio_output(audio_output)?;

        self.last_apu_sync_cycles = self.bus.state.cycles;

        Ok(())
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

    #[inline]
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
        self.bus.inputs.update_inputs(*inputs, self.bus.state.cycles, &mut self.bus.interrupts);

        self.bus.interrupts.sync(self.bus.state.cycles);
        if !self.bus.interrupts.cpu_halted() {
            self.cpu.execute_instruction(&mut self.bus);
        } else {
            self.bus.internal_cycles(1);
        }

        self.bus.sync_timers();

        // Forcibly sync the APU roughly once per line
        if self.bus.state.cycles - self.last_apu_sync_cycles >= u64::from(ppu::DOTS_PER_LINE) {
            self.drain_apu(audio_output).map_err(GbaError::Audio)?;
        }

        self.bus.sio.check_for_interrupt(self.bus.state.cycles, &mut self.bus.interrupts);

        self.bus.sync_ppu();
        if self.bus.ppu.frame_complete() {
            self.bus.ppu.clear_frame_complete();

            self.drain_apu(audio_output).map_err(GbaError::Audio)?;

            renderer
                .render_frame(
                    self.bus.ppu.frame_buffer(),
                    ppu::FRAME_SIZE,
                    self.config.aspect_ratio.to_pixel_aspect_ratio(),
                )
                .map_err(GbaError::Render)?;

            self.bus.cartridge.update_rtc_time(self.bus.state.cycles, &mut self.bus.interrupts);

            if self.bus.cartridge.take_rw_memory_dirty()
                && let Some(rw_memory) = self.bus.cartridge.rw_memory()
            {
                save_writer.persist_bytes("sav", rw_memory).map_err(GbaError::SaveWrite)?;
            }

            self.frame_count += 1;

            if let Some(rtc) = self.bus.cartridge.rtc() {
                // Limit how frequently RTC state is persisted to disk (roughly once per 10 seconds)
                if self.frame_count % 600 == 0 {
                    save_writer.persist_serialized("rtc", rtc).map_err(GbaError::SaveWrite)?;
                }
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

    #[inline]
    fn target_fps(&self) -> f64 {
        // Roughly 59.73 fps
        (crate::GBA_CLOCK_SPEED as f64)
            / f64::from(ppu::LINES_PER_FRAME)
            / f64::from(ppu::DOTS_PER_LINE)
    }

    #[inline]
    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.bus.apu.update_output_frequency(output_frequency);
    }
}
