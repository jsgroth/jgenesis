use crate::apu::Apu;
use crate::bus::Bus;
use crate::input::SnesInputs;
use crate::memory::{CpuInternalRegisters, DmaStatus, DmaUnit, Memory};
use crate::ppu::{Ppu, PpuTickEffect};
use bincode::{Decode, Encode};
use jgenesis_traits::frontend::{
    AudioOutput, Color, ConfigReload, EmulatorDebug, EmulatorTrait, PartialClone, PixelAspectRatio,
    Renderer, Resettable, SaveWriter, TakeRomFrom, TickEffect, TickableEmulator, TimingMode,
};
use std::fmt::{Debug, Display};
use thiserror::Error;
use wdc65816_emu::core::Wdc65816;

const MEMORY_REFRESH_MCLK: u64 = 536;
const MEMORY_REFRESH_CYCLES: u64 = 40;

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SnesEmulatorConfig {
    // TODO use timing mode instead of forcing NTSC
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
            apu: &mut $self.apu,
            access_master_cycles: 0,
        }
    };
}

#[derive(Debug, Clone, Encode, Decode, PartialClone)]
pub struct SnesEmulator {
    main_cpu: Wdc65816,
    cpu_registers: CpuInternalRegisters,
    dma_unit: DmaUnit,
    #[partial_clone(partial)]
    memory: Memory,
    ppu: Ppu,
    apu: Apu,
    total_master_cycles: u64,
    memory_refresh_pending: bool,
}

impl SnesEmulator {
    pub fn create(rom: Vec<u8>, config: SnesEmulatorConfig) -> Self {
        let main_cpu = Wdc65816::new();
        let cpu_registers = CpuInternalRegisters::new();
        let dma_unit = DmaUnit::new();
        let memory = Memory::from_rom(rom);
        // TODO support PAL
        let ppu = Ppu::new(TimingMode::Ntsc);
        let apu = Apu::new(TimingMode::Ntsc);

        let mut emulator = Self {
            main_cpu,
            cpu_registers,
            dma_unit,
            memory,
            ppu,
            apu,
            total_master_cycles: 0,
            memory_refresh_pending: false,
        };

        // Reset CPU so that execution starts from the right place
        emulator.main_cpu.reset(&mut new_bus!(emulator));

        emulator
    }

    pub fn cartridge_title(&mut self) -> String {
        self.memory.cartridge_title()
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
        let master_cycles_elapsed = if self.memory_refresh_pending {
            // The CPU (including DMA) halts for 40 cycles partway through every scanline so that
            // the system can refresh DRAM (used for work RAM)
            self.memory_refresh_pending = false;
            MEMORY_REFRESH_CYCLES
        } else {
            let mut bus = new_bus!(self);

            match self.dma_unit.tick(&mut bus, self.total_master_cycles) {
                DmaStatus::None => {
                    // DMA not in progress, tick CPU
                    self.main_cpu.tick(&mut bus);
                    bus.access_master_cycles
                }
                DmaStatus::InProgress { master_cycles_elapsed } => master_cycles_elapsed,
            }
        };
        assert!(master_cycles_elapsed > 0);

        let prev_scanline_mclk = self.ppu.scanline_master_cycles();
        let mut tick_effect = TickEffect::None;
        if self.ppu.tick(master_cycles_elapsed) == PpuTickEffect::FrameComplete {
            // TODO dynamic aspect ratio
            renderer
                .render_frame(
                    self.ppu.frame_buffer(),
                    self.ppu.frame_size(),
                    Some(PixelAspectRatio::try_from(1.1428571428571428).unwrap()),
                )
                .map_err(SnesError::Render)?;

            // TODO other once-per-frame events

            tick_effect = TickEffect::FrameRendered;
        }

        self.cpu_registers.update(&self.ppu);

        self.apu.tick(master_cycles_elapsed);

        // TODO run other components

        self.total_master_cycles += master_cycles_elapsed;
        if prev_scanline_mclk < MEMORY_REFRESH_MCLK
            && self.ppu.scanline_master_cycles() >= MEMORY_REFRESH_MCLK
        {
            self.memory_refresh_pending = true;
        }

        Ok(tick_effect)
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

impl TakeRomFrom for SnesEmulator {
    fn take_rom_from(&mut self, other: &mut Self) {
        todo!("take ROM from")
    }
}

impl Resettable for SnesEmulator {
    fn soft_reset(&mut self) {
        self.main_cpu.reset(&mut new_bus!(self));
        self.apu.reset();

        // TODO reset other processors and registers?
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
