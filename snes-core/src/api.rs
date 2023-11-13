use crate::apu::{Apu, ApuTickEffect};
use crate::audio::AudioResampler;
use crate::bus::Bus;
use crate::input::SnesInputs;
use crate::memory::dma::{DmaStatus, DmaUnit};
use crate::memory::{CpuInternalRegisters, Memory};
use crate::ppu::{Ppu, PpuTickEffect};
use bincode::{Decode, Encode};
use jgenesis_common::frontend::{
    AudioOutput, Color, ConfigReload, EmulatorDebug, EmulatorTrait, FrameSize, PartialClone,
    PixelAspectRatio, Renderer, Resettable, SaveWriter, TakeRomFrom, TickEffect, TickableEmulator,
    TimingMode,
};
use jgenesis_proc_macros::{EnumDisplay, EnumFromStr, FakeDecode, FakeEncode};
use std::fmt::{Debug, Display};
use std::{io, iter, mem};
use thiserror::Error;
use wdc65816_emu::core::Wdc65816;

const MEMORY_REFRESH_MCLK: u64 = 536;
const MEMORY_REFRESH_CYCLES: u64 = 40;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum SnesAspectRatio {
    #[default]
    Ntsc,
    Pal,
    SquarePixels,
    Stretched,
}

impl SnesAspectRatio {
    fn to_pixel_aspect_ratio(self, frame_size: FrameSize) -> Option<PixelAspectRatio> {
        let mut pixel_aspect_ratio = match self {
            Self::Ntsc => 8.0 / 7.0,
            Self::Pal => 11.0 / 8.0,
            Self::SquarePixels => 1.0,
            Self::Stretched => {
                return None;
            }
        };

        if frame_size.width == 512 && (frame_size.height == 224 || frame_size.height == 239) {
            // Cut pixel aspect ratio in half to account for the screen being squished horizontally
            pixel_aspect_ratio /= 2.0;
        }

        Some(PixelAspectRatio::try_from(pixel_aspect_ratio).unwrap())
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub struct SnesEmulatorConfig {
    pub forced_timing_mode: Option<TimingMode>,
    pub aspect_ratio: SnesAspectRatio,
    pub audio_60hz_hack: bool,
}

pub type CoprocessorRomFn = dyn Fn() -> Result<Vec<u8>, (io::Error, String)>;

#[derive(Default, FakeEncode, FakeDecode)]
pub struct CoprocessorRoms {
    pub dsp1: Option<Box<CoprocessorRomFn>>,
    pub dsp2: Option<Box<CoprocessorRomFn>>,
    pub dsp3: Option<Box<CoprocessorRomFn>>,
    pub dsp4: Option<Box<CoprocessorRomFn>>,
    pub st010: Option<Box<CoprocessorRomFn>>,
    pub st011: Option<Box<CoprocessorRomFn>>,
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

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("Cannot load DSP-1 cartridge because DSP-1 ROM is not configured")]
    MissingDsp1Rom,
    #[error("Cannot load DSP-2 cartridge because DSP-2 ROM is not configured")]
    MissingDsp2Rom,
    #[error("Cannot load DSP-3 cartridge because DSP-3 ROM is not configured")]
    MissingDsp3Rom,
    #[error("Cannot load DSP-4 cartridge because DSP-4 ROM is not configured")]
    MissingDsp4Rom,
    #[error("Cannot load ST010 cartridge because ST010 ROM is not configured")]
    MissingSt010Rom,
    #[error("Cannot load ST011 cartridge because ST011 ROM is not configured")]
    MissingSt011Rom,
    #[error("Failed to load required coprocessor ROM from '{path}': {source}")]
    CoprocessorRomLoad {
        #[source]
        source: io::Error,
        path: String,
    },
}

pub type LoadResult<T> = Result<T, LoadError>;

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

#[derive(Encode, Decode, PartialClone)]
pub struct SnesEmulator {
    main_cpu: Wdc65816,
    cpu_registers: CpuInternalRegisters,
    dma_unit: DmaUnit,
    #[partial_clone(partial)]
    memory: Memory,
    ppu: Ppu,
    apu: Apu,
    audio_downsampler: AudioResampler,
    total_master_cycles: u64,
    memory_refresh_pending: bool,
    timing_mode: TimingMode,
    aspect_ratio: SnesAspectRatio,
    // Stored here to enable hard reset
    #[partial_clone(default)]
    coprocessor_roms: CoprocessorRoms,
}

impl SnesEmulator {
    /// # Errors
    ///
    /// This function will return an error if it is unable to load the cartridge ROM for any reason.
    pub fn create(
        rom: Vec<u8>,
        initial_sram: Option<Vec<u8>>,
        config: SnesEmulatorConfig,
        coprocessor_roms: CoprocessorRoms,
    ) -> LoadResult<Self> {
        let main_cpu = Wdc65816::new();
        let cpu_registers = CpuInternalRegisters::new();
        let dma_unit = DmaUnit::new();
        let mut memory =
            Memory::create(rom, initial_sram, &coprocessor_roms, config.forced_timing_mode)?;

        let timing_mode =
            config.forced_timing_mode.unwrap_or_else(|| memory.cartridge_timing_mode());
        let ppu = Ppu::new(timing_mode);
        let apu = Apu::new(timing_mode, config.audio_60hz_hack);

        log::info!("Running with timing/display mode {timing_mode}");

        let mut emulator = Self {
            main_cpu,
            cpu_registers,
            dma_unit,
            memory,
            ppu,
            apu,
            audio_downsampler: AudioResampler::new(),
            total_master_cycles: 0,
            memory_refresh_pending: false,
            timing_mode,
            aspect_ratio: config.aspect_ratio,
            coprocessor_roms,
        };

        // Reset CPU so that execution starts from the right place
        emulator.main_cpu.reset(&mut new_bus!(emulator));

        Ok(emulator)
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

        // Copy WRIO from CPU to PPU for possible H/V counter latching
        self.ppu.update_wrio(self.cpu_registers.wrio_register());

        let prev_scanline_mclk = self.ppu.scanline_master_cycles();
        let mut tick_effect = TickEffect::None;
        if self.ppu.tick(master_cycles_elapsed) == PpuTickEffect::FrameComplete {
            let frame_size = self.ppu.frame_size();
            let aspect_ratio = self.aspect_ratio.to_pixel_aspect_ratio(frame_size);

            renderer
                .render_frame(self.ppu.frame_buffer(), self.ppu.frame_size(), aspect_ratio)
                .map_err(SnesError::Render)?;

            self.audio_downsampler.output_samples(audio_output).map_err(SnesError::AudioOutput)?;

            if let Some(sram) = self.memory.sram() {
                save_writer.persist_save(iter::once(sram)).map_err(SnesError::SaveWrite)?;
            }

            // TODO other once-per-frame events

            tick_effect = TickEffect::FrameRendered;
        }

        self.cpu_registers.tick(master_cycles_elapsed, &self.ppu, prev_scanline_mclk, inputs);

        if let ApuTickEffect::OutputSample(sample_l, sample_r) =
            self.apu.tick(master_cycles_elapsed)
        {
            self.audio_downsampler.collect_sample(sample_l, sample_r);
        }

        self.memory.tick(master_cycles_elapsed);

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
        let frame_size = self.ppu.frame_size();
        let aspect_ratio = self.aspect_ratio.to_pixel_aspect_ratio(frame_size);
        renderer.render_frame(self.ppu.frame_buffer(), frame_size, aspect_ratio)
    }
}

impl ConfigReload for SnesEmulator {
    type Config = SnesEmulatorConfig;

    fn reload_config(&mut self, config: &Self::Config) {
        self.aspect_ratio = config.aspect_ratio;
        self.apu.set_audio_60hz_hack(config.audio_60hz_hack);
    }
}

impl TakeRomFrom for SnesEmulator {
    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.take_rom_from(&mut other.memory);
        self.coprocessor_roms = mem::take(&mut other.coprocessor_roms);
    }
}

impl Resettable for SnesEmulator {
    fn soft_reset(&mut self) {
        log::info!("Soft resetting");

        self.main_cpu.reset(&mut new_bus!(self));
        self.cpu_registers.reset();
        self.ppu.reset();
        self.apu.reset();

        self.memory.reset();
        self.memory.write_wram_port_address_low(0);
        self.memory.write_wram_port_address_mid(0);
        self.memory.write_wram_port_address_high(0);
    }

    fn hard_reset(&mut self) {
        log::info!("Hard resetting");

        let rom = self.memory.take_rom();
        let sram = self.memory.sram().map(Vec::from);

        let coprocessor_roms = mem::take(&mut self.coprocessor_roms);
        *self = Self::create(
            rom,
            sram,
            SnesEmulatorConfig {
                forced_timing_mode: None,
                aspect_ratio: self.aspect_ratio,
                audio_60hz_hack: self.apu.get_audio_60hz_hack(),
            },
            coprocessor_roms,
        )
        .expect("Hard resetting should never fail to load");
    }
}

impl EmulatorDebug for SnesEmulator {
    const NUM_PALETTES: u32 = 16;
    const PALETTE_LEN: u32 = 16;
    const PATTERN_TABLE_LEN: u32 = 0;
    const SUPPORTS_VRAM_DEBUG: bool = false;

    fn debug_cram(&self, out: &mut [Color]) {
        self.ppu.debug_cram(out);
    }

    fn debug_vram(&self, _out: &mut [Color], _palette: u8) {
        todo!("VRAM debug")
    }
}

impl EmulatorTrait for SnesEmulator {
    type EmulatorInputs = SnesInputs;
    type EmulatorConfig = SnesEmulatorConfig;

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
