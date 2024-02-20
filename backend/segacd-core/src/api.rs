//! Sega CD public interface and main loop

use crate::audio::AudioResampler;
use crate::cddrive::CdTickEffect;
use crate::cdrom::reader::{CdRom, CdRomFileFormat};
use crate::graphics::GraphicsCoprocessor;
use crate::memory;
use crate::memory::{SegaCd, SubBus};
use crate::rf5c164::{PcmTickEffect, Rf5c164};
use bincode::{Decode, Encode};
use genesis_core::input::InputState;
use genesis_core::memory::{MainBus, MainBusSignals, MainBusWrites, Memory};
use genesis_core::vdp::{Vdp, VdpTickEffect};
use genesis_core::ym2612::{Ym2612, YmTickEffect};
use genesis_core::{GenesisAspectRatio, GenesisEmulatorConfig, GenesisInputs, GenesisRegion};
use jgenesis_common::frontend::{
    AudioOutput, Color, EmulatorTrait, PartialClone, Renderer, SaveWriter, TickEffect, TimingMode,
};
use jgenesis_proc_macros::{FakeDecode, FakeEncode};
use m68000_emu::M68000;
use smsgg_core::psg::{Psg, PsgTickEffect, PsgVersion};
use std::fmt::{Debug, Display};
use std::io;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use thiserror::Error;
use z80_emu::Z80;

const MAIN_CPU_DIVIDER: u64 = 7;
pub(crate) const SUB_CPU_DIVIDER: u64 = 4;
const Z80_DIVIDER: u64 = 15;

const NTSC_GENESIS_MASTER_CLOCK_RATE: u64 = 53_693_175;
const PAL_GENESIS_MASTER_CLOCK_RATE: u64 = 53_203_424;
pub(crate) const SEGA_CD_MASTER_CLOCK_RATE: u64 = 50_000_000;

const BIOS_LEN: usize = memory::BIOS_LEN;

#[derive(Debug, Error)]
pub enum DiscError {
    #[error("BIOS is required for Sega CD emulation")]
    MissingBios,
    #[error("BIOS must be {BIOS_LEN} bytes, was {bios_len} bytes")]
    InvalidBios { bios_len: usize },
    #[error("Unable to determine parent directory of CUE file '{0}'")]
    CueParentDir(String),
    #[error("Error parsing CUE file: {0}")]
    CueParse(String),
    #[error("Invalid/unsupported FILE line in CUE file: {0}")]
    CueInvalidFileLine(String),
    #[error("Invalid/unsupported TRACK line in CUE file: {0}")]
    CueInvalidTrackLine(String),
    #[error("Invalid/unsupported INDEX line in CUE file: {0}")]
    CueInvalidIndexLine(String),
    #[error("Invalid/unsupported PREGAP line in CUE file: {0}")]
    CueInvalidPregapLine(String),
    #[error("Unable to get file metadata for file '{path}': {source}")]
    FsMetadata {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error opening CUE file '{path}': {source}")]
    CueOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error opening BIN file '{path}': {source}")]
    BinOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("Error opening CHD file '{path}': {source}")]
    ChdOpen {
        path: String,
        #[source]
        source: io::Error,
    },
    #[error("I/O error reading from disc: {0}")]
    DiscReadIo(#[source] io::Error),
    #[error(
        "CD-ROM error detection check failed for track {track_number} sector {sector_number}; expected={expected:08X}, actual={actual:08X}"
    )]
    DiscReadInvalidChecksum { track_number: u8, sector_number: u32, expected: u32, actual: u32 },
    #[error("Error reading CHD file: {0}")]
    ChdError(#[from] chd::Error),
    #[error("Unable to parse CD-ROM metadata in CHD header: '{metadata_value}'")]
    ChdHeaderParseError { metadata_value: String },
    #[error("CHD header contains an invalid CD-ROM track list: {track_numbers:?}")]
    ChdInvalidTrackList { track_numbers: Vec<u8> },
}

pub type DiscResult<T> = Result<T, DiscError>;

#[derive(Debug, Error)]
pub enum SegaCdError<RErr, AErr, SErr> {
    #[error("Disc-related error: {0}")]
    Disc(#[from] DiscError),
    #[error("Rendering error: {0}")]
    Render(RErr),
    #[error("Audio output error: {0}")]
    Audio(AErr),
    #[error("Save write error: {0}")]
    SaveWrite(SErr),
}

pub type SegaCdResult<T, RErr, AErr, SErr> = Result<T, SegaCdError<RErr, AErr, SErr>>;

#[derive(Debug, Clone, Copy)]
pub struct SegaCdEmulatorConfig {
    pub genesis: GenesisEmulatorConfig,
    pub enable_ram_cartridge: bool,
}

#[derive(Debug, Clone, FakeEncode, FakeDecode)]
struct SaveSerializationBuffer(Vec<u8>);

impl Default for SaveSerializationBuffer {
    fn default() -> Self {
        Self(Vec::with_capacity(memory::BACKUP_RAM_LEN + memory::RAM_CARTRIDGE_LEN))
    }
}

impl Deref for SaveSerializationBuffer {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SaveSerializationBuffer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

#[derive(Debug, Encode, Decode, PartialClone)]
pub struct SegaCdEmulator {
    #[partial_clone(partial)]
    memory: Memory<SegaCd>,
    main_cpu: M68000,
    sub_cpu: M68000,
    z80: Z80,
    vdp: Vdp,
    graphics_coprocessor: GraphicsCoprocessor,
    ym2612: Ym2612,
    psg: Psg,
    pcm: Rf5c164,
    input: InputState,
    audio_resampler: AudioResampler,
    save_serialization_buffer: SaveSerializationBuffer,
    timing_mode: TimingMode,
    main_bus_writes: MainBusWrites,
    aspect_ratio: GenesisAspectRatio,
    adjust_aspect_ratio_in_2x_resolution: bool,
    disc_title: String,
    genesis_mclk_cycles: u64,
    sega_cd_mclk_cycles: u64,
    sega_cd_mclk_cycle_product: u64,
    sub_cpu_wait_cycles: u64,
}

// This is a macro instead of a function so that it only mutably borrows the needed fields
macro_rules! new_main_bus {
    ($self:expr, m68k_reset: $m68k_reset:expr) => {
        MainBus::new(
            &mut $self.memory,
            &mut $self.vdp,
            &mut $self.psg,
            &mut $self.ym2612,
            &mut $self.input,
            $self.timing_mode,
            MainBusSignals { z80_busack: $self.z80.stalled(), m68k_reset: $m68k_reset },
            std::mem::take(&mut $self.main_bus_writes),
        )
    };
}

impl SegaCdEmulator {
    /// Create a Sega CD emulator that reads a CD-ROM image from disk.
    ///
    /// # Errors
    ///
    /// Returns an error in any of the following conditions:
    /// * The BIOS is invalid
    /// * Unable to read the given CUE or CHD file
    /// * Unable to read every BIN file that is referenced in the CUE file
    /// * Unable to read boot information from the beginning of the CD-ROM data track
    #[allow(clippy::if_then_some_else_none)]
    pub fn create<P: AsRef<Path>, S: SaveWriter>(
        bios: Vec<u8>,
        rom_path: P,
        format: CdRomFileFormat,
        run_without_disc: bool,
        emulator_config: SegaCdEmulatorConfig,
        save_writer: &mut S,
    ) -> DiscResult<Self> {
        let disc = if !run_without_disc { Some(CdRom::open(rom_path, format)?) } else { None };

        Self::create_from_disc(bios, disc, emulator_config, save_writer)
    }

    /// Create a Sega CD emulator that reads a CD-ROM image from an in-memory CHD image.
    ///
    /// # Errors
    ///
    /// Returns an error if the BIOS is invalid, the CHD image is invalid, or the emulator is unable
    /// to read boot information from the beginning of the CD-ROM data track.
    pub fn create_in_memory<S: SaveWriter>(
        bios: Vec<u8>,
        chd_bytes: Vec<u8>,
        emulator_config: SegaCdEmulatorConfig,
        save_writer: &mut S,
    ) -> DiscResult<Self> {
        let disc = CdRom::open_chd_in_memory(chd_bytes)?;

        Self::create_from_disc(bios, Some(disc), emulator_config, save_writer)
    }

    fn create_from_disc<S: SaveWriter>(
        bios: Vec<u8>,
        disc: Option<CdRom>,
        emulator_config: SegaCdEmulatorConfig,
        save_writer: &mut S,
    ) -> DiscResult<Self> {
        if bios.len() != BIOS_LEN {
            return Err(DiscError::InvalidBios { bios_len: bios.len() });
        }

        let initial_backup_ram = save_writer.load_bytes("sav").ok();
        let mut sega_cd = SegaCd::new(
            bios,
            disc,
            initial_backup_ram,
            emulator_config.enable_ram_cartridge,
            emulator_config.genesis.forced_region,
        )?;
        let disc_title = sega_cd.disc_title()?.unwrap_or("(no disc)".into());

        let memory = Memory::new(sega_cd);
        let timing_mode =
            emulator_config.genesis.forced_timing_mode.unwrap_or_else(|| {
                match memory.hardware_region() {
                    GenesisRegion::Americas | GenesisRegion::Japan => TimingMode::Ntsc,
                    GenesisRegion::Europe => TimingMode::Pal,
                }
            });

        log::info!("Running with timing/display mode: {timing_mode}");

        let main_cpu = M68000::builder().allow_tas_writes(false).name("Main".into()).build();
        let sub_cpu = M68000::builder().name("Sub".into()).build();
        let z80 = Z80::new();
        let vdp = Vdp::new(timing_mode, emulator_config.genesis.to_vdp_config());
        let graphics_coprocessor = GraphicsCoprocessor::new();
        let ym2612 = Ym2612::new(emulator_config.genesis.quantize_ym2612_output);
        let psg = Psg::new(PsgVersion::Standard);
        let pcm = Rf5c164::new();
        let input = InputState::new();

        let audio_resampler = AudioResampler::new(timing_mode);
        let mut emulator = Self {
            memory,
            main_cpu,
            sub_cpu,
            z80,
            vdp,
            graphics_coprocessor,
            ym2612,
            psg,
            pcm,
            input,
            audio_resampler,
            save_serialization_buffer: SaveSerializationBuffer::default(),
            timing_mode,
            main_bus_writes: MainBusWrites::new(),
            aspect_ratio: emulator_config.genesis.aspect_ratio,
            adjust_aspect_ratio_in_2x_resolution: emulator_config
                .genesis
                .adjust_aspect_ratio_in_2x_resolution,
            disc_title,
            genesis_mclk_cycles: 0,
            sega_cd_mclk_cycles: 0,
            sega_cd_mclk_cycle_product: 0,
            sub_cpu_wait_cycles: 0,
        };

        // Reset main CPU so that execution starts from the right place
        emulator.main_cpu.execute_instruction(&mut new_main_bus!(emulator, m68k_reset: true));

        Ok(emulator)
    }

    #[inline]
    fn tick_sub_cpu(&mut self, mut sub_cpu_cycles: u64) {
        while sub_cpu_cycles >= self.sub_cpu_wait_cycles {
            let wait_cycles = self.sub_cpu_wait_cycles;
            let mut bus =
                SubBus::new(&mut self.memory, &mut self.graphics_coprocessor, &mut self.pcm);
            self.sub_cpu_wait_cycles = self.sub_cpu.execute_instruction(&mut bus).into();
            sub_cpu_cycles -= wait_cycles;
        }

        self.sub_cpu_wait_cycles -= sub_cpu_cycles;
    }

    fn render_frame<R: Renderer>(&self, renderer: &mut R) -> Result<(), R::Err> {
        genesis_core::render_frame(
            &self.vdp,
            self.aspect_ratio,
            self.adjust_aspect_ratio_in_2x_resolution,
            renderer,
        )
    }

    #[must_use]
    pub fn disc_title(&self) -> &str {
        &self.disc_title
    }

    pub fn remove_disc(&mut self) {
        self.memory.medium_mut().remove_disc();
        self.disc_title = "(no disc)".into();
    }

    /// # Errors
    ///
    /// This method will return an error if the disc drive is unable to load the disc.
    pub fn change_disc<P: AsRef<Path>>(
        &mut self,
        rom_path: P,
        format: CdRomFileFormat,
    ) -> DiscResult<()> {
        let sega_cd = self.memory.medium_mut();
        sega_cd.change_disc(rom_path, format)?;
        self.disc_title = sega_cd.disc_title()?.unwrap_or_else(|| "(no disc)".into());

        Ok(())
    }

    pub fn copy_cram(&self, out: &mut [Color]) {
        self.vdp.copy_cram(out);
    }

    pub fn copy_vram(&self, out: &mut [Color], palette: u8, row_len: usize) {
        self.vdp.copy_vram(out, palette, row_len);
    }
}

impl EmulatorTrait for SegaCdEmulator {
    type Inputs = GenesisInputs;
    type Config = SegaCdEmulatorConfig;

    type Err<
        RErr: Debug + Display + Send + Sync + 'static,
        AErr: Debug + Display + Send + Sync + 'static,
        SErr: Debug + Display + Send + Sync + 'static,
    > = SegaCdError<RErr, AErr, SErr>;

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
        let mut main_bus = new_main_bus!(self, m68k_reset: false);

        // Main 68000
        let main_cpu_cycles = self.main_cpu.execute_instruction(&mut main_bus);

        let genesis_mclk_elapsed = u64::from(main_cpu_cycles) * MAIN_CPU_DIVIDER;
        let z80_cycles = (self.genesis_mclk_cycles + genesis_mclk_elapsed) / Z80_DIVIDER
            - self.genesis_mclk_cycles / Z80_DIVIDER;
        self.genesis_mclk_cycles += genesis_mclk_elapsed;

        // Z80
        for _ in 0..z80_cycles {
            self.z80.tick(&mut main_bus);
        }

        self.main_bus_writes = main_bus.take_writes();

        self.sega_cd_mclk_cycle_product += genesis_mclk_elapsed * SEGA_CD_MASTER_CLOCK_RATE;
        let scd_mclk_elapsed = match self.timing_mode {
            TimingMode::Ntsc => {
                let elapsed = self.sega_cd_mclk_cycle_product / NTSC_GENESIS_MASTER_CLOCK_RATE;
                self.sega_cd_mclk_cycle_product -= elapsed * NTSC_GENESIS_MASTER_CLOCK_RATE;
                elapsed
            }
            TimingMode::Pal => {
                let elapsed = self.sega_cd_mclk_cycle_product / PAL_GENESIS_MASTER_CLOCK_RATE;
                self.sega_cd_mclk_cycle_product -= elapsed * PAL_GENESIS_MASTER_CLOCK_RATE;
                elapsed
            }
        };

        let prev_scd_mclk_cycles = self.sega_cd_mclk_cycles;
        self.sega_cd_mclk_cycles += scd_mclk_elapsed;

        let sub_cpu_cycles =
            self.sega_cd_mclk_cycles / SUB_CPU_DIVIDER - prev_scd_mclk_cycles / SUB_CPU_DIVIDER;
        let elapsed_scd_mclk_cycles = self.sega_cd_mclk_cycles - prev_scd_mclk_cycles;

        // Disc drive and timer/stopwatch
        let sega_cd = self.memory.medium_mut();
        if let CdTickEffect::OutputAudioSample(sample_l, sample_r) =
            sega_cd.tick(elapsed_scd_mclk_cycles, &mut self.pcm)?
        {
            self.audio_resampler.collect_cd_sample(sample_l, sample_r);
        }

        // Graphics ASIC
        let graphics_interrupt_enabled = sega_cd.graphics_interrupt_enabled();
        self.graphics_coprocessor.tick(
            elapsed_scd_mclk_cycles,
            sega_cd.word_ram_mut(),
            graphics_interrupt_enabled,
        );

        // Sub 68000
        self.tick_sub_cpu(sub_cpu_cycles);

        // Apply main CPU writes after ticking the sub CPU; this fixes random freezing in Silpheed
        self.main_bus_writes = new_main_bus!(self, m68k_reset: false).apply_writes();

        // Input state (for 6-button controller reset)
        self.input.tick(main_cpu_cycles);

        // PSG
        for _ in 0..z80_cycles {
            if self.psg.tick() == PsgTickEffect::Clocked {
                let (psg_sample_l, psg_sample_r) = self.psg.sample();
                self.audio_resampler.collect_psg_sample(psg_sample_l, psg_sample_r);
            }
        }

        // YM2612
        for _ in 0..main_cpu_cycles {
            if self.ym2612.tick() == YmTickEffect::OutputSample {
                let (ym2612_sample_l, ym2612_sample_r) = self.ym2612.sample();
                self.audio_resampler.collect_ym2612_sample(ym2612_sample_l, ym2612_sample_r);
            }
        }

        // RF5C164
        if self.pcm.tick(sub_cpu_cycles) == PcmTickEffect::Clocked {
            let (pcm_sample_l, pcm_sample_r) = self.pcm.sample();
            self.audio_resampler.collect_pcm_sample(pcm_sample_l, pcm_sample_r);
        }

        // Output any audio samples that are queued up
        self.audio_resampler.output_samples(audio_output).map_err(SegaCdError::Audio)?;

        // VDP
        if self.vdp.tick(genesis_mclk_elapsed, &mut self.memory) == VdpTickEffect::FrameComplete {
            self.render_frame(renderer).map_err(SegaCdError::Render)?;

            self.input.set_inputs(inputs);

            if self.memory.medium_mut().get_and_clear_backup_ram_dirty_bit() {
                let sega_cd = self.memory.medium();

                self.save_serialization_buffer.clear();
                self.save_serialization_buffer.extend(sega_cd.backup_ram());
                self.save_serialization_buffer.extend(sega_cd.ram_cartridge());

                save_writer
                    .persist_bytes("sav", &self.save_serialization_buffer)
                    .map_err(SegaCdError::SaveWrite)?;
            }

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
        self.aspect_ratio = config.genesis.aspect_ratio;
        self.adjust_aspect_ratio_in_2x_resolution =
            config.genesis.adjust_aspect_ratio_in_2x_resolution;
        self.vdp.reload_config(config.genesis.to_vdp_config());
        self.ym2612.set_quantize_output(config.genesis.quantize_ym2612_output);
        self.input.reload_config(config.genesis);

        let sega_cd = self.memory.medium_mut();
        sega_cd.set_forced_region(config.genesis.forced_region);
        sega_cd.set_enable_ram_cartridge(config.enable_ram_cartridge);
    }

    fn take_rom_from(&mut self, other: &mut Self) {
        self.memory.medium_mut().take_rom_from(other.memory.medium_mut());
    }

    fn soft_reset(&mut self) {
        // Reset main CPU
        self.main_cpu.execute_instruction(&mut new_main_bus!(self, m68k_reset: true));
        self.memory.reset_z80_signals();

        self.ym2612.reset();
        self.pcm.disable();

        self.memory.medium_mut().reset();
    }

    fn hard_reset<S: SaveWriter>(&mut self, save_writer: &mut S) {
        let sega_cd = self.memory.medium_mut();
        let bios = Vec::from(sega_cd.bios());
        let disc = sega_cd.take_cdrom();
        let forced_region = sega_cd.forced_region();
        let enable_ram_cartridge = sega_cd.get_enable_ram_cartridge();
        let vdp_config = self.vdp.config();
        let (p1_controller_type, p2_controller_type) = self.input.controller_types();

        *self = Self::create_from_disc(
            bios,
            disc,
            SegaCdEmulatorConfig {
                genesis: GenesisEmulatorConfig {
                    forced_timing_mode: Some(self.timing_mode),
                    forced_region,
                    aspect_ratio: self.aspect_ratio,
                    adjust_aspect_ratio_in_2x_resolution: self.adjust_aspect_ratio_in_2x_resolution,
                    remove_sprite_limits: !vdp_config.enforce_sprite_limits,
                    emulate_non_linear_vdp_dac: vdp_config.emulate_non_linear_dac,
                    render_vertical_border: vdp_config.render_vertical_border,
                    render_horizontal_border: vdp_config.render_horizontal_border,
                    quantize_ym2612_output: self.ym2612.get_quantize_output(),
                    p1_controller_type,
                    p2_controller_type,
                },
                enable_ram_cartridge,
            },
            save_writer,
        )
        .expect("Hard reset should not cause an I/O error");
    }

    fn timing_mode(&self) -> TimingMode {
        self.timing_mode
    }
}
