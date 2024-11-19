use crate::bus::Bus;
use crate::cartridge::Cartridge;
use crate::control::ControlRegisters;
use crate::input::GbaInputs;
use crate::memory::Memory;
use crate::ppu::{Ppu, PpuTickEffect};
use crate::{bus, ppu};
use arm7tdmi_emu::{Arm7Tdmi, Arm7TdmiResetArgs, CpuMode};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, EmulatorTrait, PartialClone, PixelAspectRatio, Renderer, SaveWriter, TickEffect,
    TickResult, TimingMode,
};
use std::fmt::{Debug, Display};
use std::mem;
use thiserror::Error;

// 1 PPU cycle per 4 CPU cycles
const PPU_DIVIDER: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode)]
pub struct GbaEmulatorConfig {}

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
    #[partial_clone(partial)]
    memory: Memory,
    control: ControlRegisters,
    ppu_mclk_counter: u32,
}

impl GameBoyAdvanceEmulator {
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
            control: ControlRegisters::new(),
            memory,
            ppu_mclk_counter: 0,
        };

        emulator.cpu.manual_reset(
            Arm7TdmiResetArgs {
                pc: bus::CARTRIDGE_ROM_0_START,
                sp_usr: 0x03007F00,
                sp_svc: 0x03007FE0,
                sp_irq: 0x03007FA0,
                mode: CpuMode::System,
            },
            &mut Bus {
                ppu: &mut emulator.ppu,
                memory: &mut emulator.memory,
                control: &mut emulator.control,
                inputs: GbaInputs::default(),
            },
        );

        Ok(emulator)
    }

    fn render_frame<R: Renderer>(&self, renderer: &mut R) -> Result<(), R::Err> {
        renderer.render_frame(
            self.ppu.frame_buffer(),
            ppu::FRAME_SIZE,
            Some(PixelAspectRatio::SQUARE),
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
        let cpu_cycles = self.cpu.execute_instruction(&mut Bus {
            ppu: &mut self.ppu,
            memory: &mut self.memory,
            control: &mut self.control,
            inputs: *inputs,
        });

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

    fn reload_config(&mut self, config: &Self::Config) {}

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.cartridge.rom = mem::take(&mut other.memory.cartridge.rom);
    }

    fn soft_reset(&mut self) {
        todo!("soft reset")
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        todo!("hard reset")
    }

    fn timing_mode(&self) -> TimingMode {
        TimingMode::Ntsc
    }

    fn target_fps(&self) -> f64 {
        // TODO figure out actual refresh rate
        60.0
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        // TODO audio resampling
    }
}
