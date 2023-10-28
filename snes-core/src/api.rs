use crate::bus::Bus;
use crate::input::SnesInputs;
use crate::memory::{CpuInternalRegisters, DmaStatus, DmaUnit, Memory};
use crate::ppu::Ppu;
use bincode::{Decode, Encode};
use jgenesis_traits::frontend::{
    AudioOutput, Color, ConfigReload, EmulatorDebug, EmulatorTrait, PartialClone, Renderer,
    Resettable, SaveWriter, TakeRomFrom, TickEffect, TickableEmulator, TimingMode,
};
use std::fmt::{Debug, Display};
use thiserror::Error;
use wdc65816_emu::core::Wdc65816;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SnesEmulatorConfig {
    pub forced_timing_mode: Option<TimingMode>,
}

#[derive(Debug, Error)]
pub enum SnesError<RErr, AErr, SErr> {
    #[error("Error rendering frame: {0}")]
    Render(RErr),
    #[error("Error outputting audio samples: {0}")]
    AudioOutput(AErr),
    #[error("Error persisting save file: {0}")]
    SaveWrite(SErr),
}

macro_rules! new_bus {
    ($self:expr) => {
        Bus {
            memory: &mut $self.memory,
            cpu_registers: &mut $self.cpu_registers,
            ppu: &mut $self.ppu,
            access_master_cycles: 0,
        }
    };
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct SnesEmulator {
    main_cpu: Wdc65816,
    cpu_registers: CpuInternalRegisters,
    dma_unit: DmaUnit,
    memory: Memory,
    ppu: Ppu,
    total_master_cycles: u64,
}

impl SnesEmulator {
    pub fn create(rom: Vec<u8>, _config: SnesEmulatorConfig) -> Self {
        let main_cpu = Wdc65816::new();
        let cpu_registers = CpuInternalRegisters::new();
        let dma_unit = DmaUnit::new();
        let memory = Memory::from_rom(rom);
        let ppu = Ppu::new();

        let mut emulator =
            Self { main_cpu, cpu_registers, dma_unit, memory, ppu, total_master_cycles: 0 };

        // Reset CPU so that execution starts from the right place
        emulator.main_cpu.reset(&mut new_bus!(emulator));

        emulator
    }
}

impl TickableEmulator for SnesEmulator {
    type Inputs = SnesInputs;
    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = SnesError<RErr, AErr, SErr>;

    fn tick<R, A, S>(
        &mut self,
        renderer: &mut R,
        audio_output: &mut A,
        inputs: &Self::Inputs,
        save_writer: &mut S,
    ) -> Result<TickEffect, Self::Err<R::Err, A::Err, S::Err>>
    where
        R: Renderer,
        R::Err: Debug + Display + Send + Sync + 'static,
        A: AudioOutput,
        A::Err: Debug + Display + Send + Sync + 'static,
        S: SaveWriter,
        S::Err: Debug + Display + Send + Sync + 'static,
    {
        let mut bus = new_bus!(self);

        let master_cycles_elapsed = match self.dma_unit.tick(&mut bus, self.total_master_cycles) {
            DmaStatus::None => {
                // DMA not in progress, tick CPU
                self.main_cpu.tick(&mut bus);
                bus.access_master_cycles
            }
            DmaStatus::InProgress { master_cycles_elapsed } => master_cycles_elapsed,
        };
        assert!(master_cycles_elapsed > 0);

        // TODO run other components

        Ok(TickEffect::None)
    }

    fn force_render<R>(&mut self, renderer: &mut R) -> Result<(), R::Err>
    where
        R: Renderer,
    {
        todo!("force render")
    }
}

impl ConfigReload for SnesEmulator {
    type Config = SnesEmulatorConfig;

    fn reload_config(&mut self, config: &Self::Config) {
        todo!("reload config")
    }
}

impl PartialClone for SnesEmulator {
    fn partial_clone(&self) -> Self {
        todo!("partial clone")
    }
}

impl TakeRomFrom for SnesEmulator {
    fn take_rom_from(&mut self, other: &mut Self) {
        todo!("take ROM from")
    }
}

impl Resettable for SnesEmulator {
    fn soft_reset(&mut self) {
        todo!("soft reset")
    }

    fn hard_reset(&mut self) {
        todo!("hard reset")
    }
}

impl EmulatorDebug for SnesEmulator {
    const NUM_PALETTES: u32 = 0;
    const PALETTE_LEN: u32 = 0;
    const PATTERN_TABLE_LEN: u32 = 0;

    fn debug_cram(&self, out: &mut [Color]) {
        todo!("CRAM debug")
    }

    fn debug_vram(&self, out: &mut [Color], palette: u8) {
        todo!("VRAM debug")
    }
}

impl EmulatorTrait for SnesEmulator {
    type EmulatorInputs = SnesInputs;
    type EmulatorConfig = SnesEmulatorConfig;

    fn timing_mode(&self) -> TimingMode {
        todo!("timing mode")
    }
}
