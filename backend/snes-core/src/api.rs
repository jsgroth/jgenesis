//! SNES public interface and main loop

use crate::apu::{Apu, ApuTickEffect};
use crate::audio::AudioResampler;
use crate::bus::Bus;
use crate::input::{SnesButton, SnesInputs};
use crate::memory::dma::{DmaStatus, DmaUnit};
use crate::memory::{CpuInternalRegisters, Memory};
use crate::ppu::{Ppu, PpuTickEffect};
use bincode::error::EncodeError;
use bincode::{Decode, Encode};
use crc::Crc;
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorTrait, FrameSize, PartialClone, PixelAspectRatio, Renderer,
    SaveWriter, TickEffect, TimingMode,
};
use jgenesis_proc_macros::{
    ConfigDisplay, EnumAll, EnumDisplay, EnumFromStr, FakeDecode, FakeEncode,
};
use std::fmt::{Debug, Display};
use std::num::NonZeroU64;
use std::{io, mem};
use thiserror::Error;
use wdc65816_emu::core::Wdc65816;
use wdc65816_emu::traits::BusInterface;

const MEMORY_REFRESH_MCLK: u64 = 536;
const MEMORY_REFRESH_CYCLES: u64 = 40;

const CRC: Crc<u32> = Crc::<u32>::new(&crc::CRC_32_ISO_HDLC);

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
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

        if frame_size.width == 512 && frame_size.height < 240 {
            // Cut pixel aspect ratio in half to account for the screen being squished horizontally
            pixel_aspect_ratio *= 0.5;
        }

        if frame_size.width == 256 && frame_size.height >= 240 {
            // Double pixel aspect ratio to account for the screen being stretched horizontally
            pixel_aspect_ratio *= 2.0;
        }

        Some(PixelAspectRatio::try_from(pixel_aspect_ratio).unwrap())
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Encode, Decode, EnumDisplay, EnumFromStr, EnumAll,
)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "clap", derive(jgenesis_proc_macros::CustomValueEnum))]
pub enum AudioInterpolationMode {
    #[default]
    Gaussian,
    Hermite,
}

#[derive(Debug, Clone, Copy, Encode, Decode, ConfigDisplay)]
pub struct SnesEmulatorConfig {
    pub forced_timing_mode: Option<TimingMode>,
    pub aspect_ratio: SnesAspectRatio,
    pub deinterlace: bool,
    pub audio_interpolation: AudioInterpolationMode,
    pub audio_60hz_hack: bool,
    pub gsu_overclock_factor: NonZeroU64,
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

impl CoprocessorRoms {
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }
}

#[derive(Debug, Error)]
pub enum SnesError<RErr, AErr, SErr> {
    #[error("Error rendering frame: {0}")]
    Render(RErr),
    #[error("Error outputting audio samples: {0}")]
    AudioOutput(AErr),
    #[error("Error persisting save file: {0}")]
    SaveWrite(SErr),
    #[error("Error encoding save file bytes: {0}")]
    SaveEncode(#[from] EncodeError),
}

#[derive(Debug, Error)]
pub enum SnesLoadError {
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

pub type SnesLoadResult<T> = Result<T, SnesLoadError>;

macro_rules! new_bus {
    ($self:expr) => {
        Bus {
            memory: &mut $self.memory,
            cpu_registers: &mut $self.cpu_registers,
            ppu: &mut $self.ppu,
            apu: &mut $self.apu,
            latched_interrupts: $self.latched_interrupts,
            access_master_cycles: 0,
            pending_write: None,
        }
    };
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
pub(crate) struct LatchedInterrupts {
    pub nmi: bool,
    pub irq: bool,
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
    audio_resampler: AudioResampler,
    total_master_cycles: u64,
    latched_interrupts: Option<LatchedInterrupts>,
    memory_refresh_pending: bool,
    timing_mode: TimingMode,
    aspect_ratio: SnesAspectRatio,
    frame_count: u64,
    last_sram_checksum: u32,
    // Following fields only stored here to enable hard reset
    #[partial_clone(default)]
    coprocessor_roms: CoprocessorRoms,
    emulator_config: SnesEmulatorConfig,
}

impl SnesEmulator {
    /// # Errors
    ///
    /// This function will return an error if it is unable to load the cartridge ROM for any reason.
    pub fn create<S: SaveWriter>(
        rom: Vec<u8>,
        config: SnesEmulatorConfig,
        coprocessor_roms: CoprocessorRoms,
        save_writer: &mut S,
    ) -> SnesLoadResult<Self> {
        let main_cpu = Wdc65816::new();
        let cpu_registers = CpuInternalRegisters::new();
        let dma_unit = DmaUnit::new();

        let initial_sram = save_writer.load_bytes("sav").ok();
        let sram_checksum = initial_sram.as_ref().map_or(0, |sram| CRC.checksum(sram));
        let mut memory = Memory::create(
            rom,
            initial_sram,
            &coprocessor_roms,
            config.forced_timing_mode,
            config.gsu_overclock_factor,
            save_writer,
        )?;

        let timing_mode =
            config.forced_timing_mode.unwrap_or_else(|| memory.cartridge_timing_mode());
        let ppu = Ppu::new(timing_mode, config);
        let apu = Apu::new(timing_mode, config);

        log::info!("Running with timing/display mode {timing_mode}");

        let mut emulator = Self {
            main_cpu,
            cpu_registers,
            dma_unit,
            memory,
            ppu,
            apu,
            audio_resampler: AudioResampler::new(),
            total_master_cycles: 0,
            latched_interrupts: None,
            memory_refresh_pending: false,
            timing_mode,
            aspect_ratio: config.aspect_ratio,
            frame_count: 0,
            last_sram_checksum: sram_checksum,
            coprocessor_roms,
            emulator_config: config,
        };

        // Reset CPU so that execution starts from the right place
        emulator.main_cpu.reset(&mut new_bus!(emulator));

        Ok(emulator)
    }

    #[must_use]
    pub fn cartridge_title(&mut self) -> String {
        self.memory.cartridge_title()
    }

    #[inline]
    #[must_use]
    pub fn has_sram(&self) -> bool {
        self.memory.has_battery_backed_sram()
    }

    pub fn copy_cgram(&self, out: &mut [Color]) {
        self.ppu.copy_cgram(out);
    }

    pub fn copy_vram_2bpp(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.ppu.copy_vram_2bpp(out, palette, row_len);
    }

    pub fn copy_vram_4bpp(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.ppu.copy_vram_4bpp(out, palette, row_len);
    }

    pub fn copy_vram_8bpp(&self, out: &mut [Color], row_len: usize) {
        self.ppu.copy_vram_8bpp(out, row_len);
    }

    pub fn copy_vram_mode7(&self, out: &mut [Color], row_len: usize) {
        self.ppu.copy_vram_mode7(out, row_len);
    }
}

impl EmulatorTrait for SnesEmulator {
    type Button = SnesButton;
    type Inputs = SnesInputs;
    type Config = SnesEmulatorConfig;

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
        let (master_cycles_elapsed, pending_write) = if self.memory_refresh_pending {
            // The CPU (including DMA) halts for 40 cycles partway through every scanline so that
            // the system can refresh DRAM (used for work RAM)
            self.memory_refresh_pending = false;
            (MEMORY_REFRESH_CYCLES, None)
        } else {
            let mut bus = new_bus!(self);

            match self.dma_unit.tick(&mut bus, self.total_master_cycles) {
                DmaStatus::None => {
                    // DMA not in progress, tick CPU
                    self.main_cpu.tick(&mut bus);
                    self.latched_interrupts = None;

                    (bus.access_master_cycles, bus.pending_write)
                }
                DmaStatus::InProgress { master_cycles_elapsed } => {
                    // Latch interrupt lines at the start of DMA to emulate interrupt tests being
                    // delayed by one cycle after the DMA ends.
                    // Wild Guns depends on this
                    if self.latched_interrupts.is_none() {
                        self.latched_interrupts =
                            Some(LatchedInterrupts { nmi: bus.nmi(), irq: bus.irq() });
                    }

                    (master_cycles_elapsed, None)
                }
            }
        };
        debug_assert!(master_cycles_elapsed > 0);

        // Copy WRIO from CPU to PPU for possible H/V counter latching
        self.ppu.update_wrio(self.cpu_registers.wrio_register());

        // Possibly latch H/V counter from the controller (e.g. Super Scope)
        if let Some((h, v)) = self.cpu_registers.controller_hv_latch() {
            self.ppu.update_controller_hv_latch(h, v, master_cycles_elapsed);
        }

        if let ApuTickEffect::OutputSample(sample_l, sample_r) =
            self.apu.tick(master_cycles_elapsed)
        {
            self.audio_resampler.collect_sample(sample_l, sample_r);
        }

        self.audio_resampler.output_samples(audio_output).map_err(SnesError::AudioOutput)?;

        self.memory.tick(master_cycles_elapsed);

        let prev_scanline_mclk = self.ppu.scanline_master_cycles();
        let mut tick_effect = TickEffect::None;
        if self.ppu.tick(master_cycles_elapsed) == PpuTickEffect::FrameComplete {
            let frame_size = self.ppu.frame_size();
            let aspect_ratio = self.aspect_ratio.to_pixel_aspect_ratio(frame_size);

            renderer
                .render_frame(self.ppu.frame_buffer(), self.ppu.frame_size(), aspect_ratio)
                .map_err(SnesError::Render)?;

            // Only persist SRAM if it's changed since the last write, and only check ~twice per
            // second because of the checksum calculation
            if self.memory.has_battery_backed_sram() {
                if let Some(sram) = self.memory.sram() {
                    if self.frame_count % 30 == 0 {
                        let checksum = CRC.checksum(sram);
                        if checksum != self.last_sram_checksum {
                            save_writer.persist_bytes("sav", sram).map_err(SnesError::SaveWrite)?;
                            self.memory
                                .write_auxiliary_save_files(save_writer)
                                .map_err(SnesError::SaveWrite)?;

                            self.last_sram_checksum = checksum;
                        }
                    }
                }
            }

            self.frame_count += 1;
            tick_effect = TickEffect::FrameRendered;
        }

        self.cpu_registers.tick(master_cycles_elapsed, &self.ppu, prev_scanline_mclk, inputs);

        // CPU reads are applied before advancing other components but CPU writes are applied after.
        // This fixes freezing in Rendering Ranger R2
        if let Some((address, value)) = pending_write {
            new_bus!(self).apply_write(address, value);
        }

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

    fn reload_config(&mut self, config: &Self::Config) {
        self.aspect_ratio = config.aspect_ratio;
        self.ppu.update_config(*config);
        self.apu.update_config(*config);
        self.memory.update_gsu_overclock_factor(config.gsu_overclock_factor);

        self.emulator_config = *config;
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.take_rom_from(&mut other.memory);
        self.coprocessor_roms = mem::take(&mut other.coprocessor_roms);
    }

    fn soft_reset(&mut self) {
        log::info!("Soft resetting");

        // Reset memory before CPU because some coprocessors (Super FX) block access to the
        // RESET interrupt vector while the coprocessor is running
        self.memory.reset();

        self.main_cpu.reset(&mut new_bus!(self));
        self.cpu_registers.reset();
        self.ppu.reset();
        self.apu.reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        log::info!("Hard resetting");

        let rom = self.memory.take_rom();

        let coprocessor_roms = mem::take(&mut self.coprocessor_roms);
        *self = Self::create(rom, self.emulator_config, coprocessor_roms, save_writer)
            .expect("Hard resetting should never fail to load");
    }

    fn target_fps(&self) -> f64 {
        match (self.timing_mode, self.emulator_config.audio_60hz_hack) {
            (TimingMode::Ntsc, true) => 60.0,
            (TimingMode::Ntsc, false) => 60.0988,
            (TimingMode::Pal, true) => 50.0,
            (TimingMode::Pal, false) => 50.007,
        }
    }

    fn update_audio_output_frequency(&mut self, output_frequency: u64) {
        self.audio_resampler.update_output_frequency(output_frequency);
    }
}
